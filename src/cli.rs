//! CLI 인터페이스

use crate::archive::SessionArchiver;
use crate::compact::SessionCompactor;
use crate::config::Config;
use crate::db::SessionDb;
use crate::error::Result;
use crate::i18n;
use crate::scanner::CodexScanner;
use crate::summary::SummaryBuilder;
use crate::types::{Backend, SessionInfo};
use clap::{Parser, Subcommand};

/// Session Butler - 세션 로그 관리 도구
#[derive(Parser, Debug)]
#[command(name = "session-butler")]
#[command(author = "Kim Jeongjin")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(
    about = "Compress, archive, and summarize session logs",
    long_about = "Manages Codex sessions (rollout-*.jsonl) via scan/archive/restore/compact, \
and summarizes session logs (session_*.json) into a searchable knowledge base.\n\
Backend activation precedence: config file → env (CODEX_ENABLED/HERMES_ENABLED) → \
--no-codex/--no-hermes"
)]
pub struct Cli {
    /// Command
    #[command(subcommand)]
    pub command: Commands,

    /// Codex sessions directory
    #[arg(short = 'C', long, global = true)]
    pub codex_sessions: Option<String>,

    /// Session-summary backend directory (session_*.json)
    #[arg(short = 'H', long, global = true)]
    pub hermes_sessions: Option<String>,

    /// Codex archive directory
    #[arg(short = 'A', long, global = true)]
    pub codex_archive: Option<String>,

    /// Codex index DB path
    #[arg(short = 'I', long, global = true)]
    pub codex_index_db: Option<String>,

    /// Summary layer JSON path
    #[arg(short = 'S', long, global = true)]
    pub summary_layer: Option<String>,

    /// FTS5 index JSON path
    #[arg(short = 'F', long, global = true)]
    pub fts5_index: Option<String>,

    /// Verbose output
    #[arg(short = 'v', long, global = true)]
    pub verbose: bool,

    /// Disable Codex backend (skips scan/archive/restore/list/stats/compact)
    #[arg(long, global = true)]
    pub no_codex: bool,

    /// Disable the session-summary backend (skips summarize)
    #[arg(long, global = true)]
    pub no_hermes: bool,
}

/// 세부 명령
#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Codex: scan & index sessions (FTS5 full-text index)
    Scan {
        /// Generate analysis report
        #[arg(long)]
        analyze: bool,
    },

    /// Codex: compress sessions (--move deletes originals, --skip-scan skips pre-scan)
    Archive {
        /// Retention window: keep recent N days, compress older sessions
        #[arg(short = 'd', long, default_value = "30")]
        days: u64,

        /// Dry-run (no actual execution)
        #[arg(long)]
        dry_run: bool,

        /// Delete original .jsonl after compress (move). Requires scan (conflicts with --skip-scan)
        #[arg(long = "move", conflicts_with = "skip_scan")]
        move_: bool,

        /// Skip pre-archive scan (index refresh)
        #[arg(long)]
        skip_scan: bool,
    },

    /// Codex: restore sessions (from DB archive index, --purge deletes .zst)
    Restore {
        /// Session ID to restore
        #[arg(long)]
        session_id: Option<String>,

        /// Restore all
        #[arg(long)]
        all: bool,

        /// Restore archived sessions from the last N days
        #[arg(short = 'd', long, default_value = "30")]
        days: u64,

        /// Dry-run
        #[arg(long)]
        dry_run: bool,

        /// Delete .zst after restore + clear archived flag
        #[arg(long)]
        purge: bool,
    },

    /// Codex: list sessions
    List {
        /// Show sessions from the last N days
        #[arg(short = 'd', long, default_value = "30")]
        days: u64,

        /// JSON output
        #[arg(long)]
        json: bool,
    },

    /// Codex: statistics
    Stats {
        /// Stats from the last N days
        #[arg(short = 'd', long, default_value = "30")]
        days: u64,
    },

    /// Codex: usage insights (tools/skills, projects, trends, words) from indexed data
    Insights {
        /// Time window in days (0 = all time)
        #[arg(short = 'd', long, default_value = "0")]
        days: u64,

        /// Top-N per ranking
        #[arg(short = 'n', long, default_value = "15")]
        top: usize,

        /// Time-bucket granularity for the trend table
        #[arg(long, value_enum, default_value_t = crate::insights::Granularity::Month)]
        by: crate::insights::Granularity,

        /// Word analysis source: full conversation body, or first prompt only
        #[arg(long, value_enum, default_value_t = crate::insights::WordsSource::Full)]
        words: crate::insights::WordsSource,

        /// JSON output
        #[arg(long)]
        json: bool,
    },

    /// Codex: compaction + sensitive-info (.env/token/key) scan
    Compact {
        /// Compact sessions older than N days (0 = all before today)
        #[arg(short = 'd', long, default_value = "0")]
        days: u64,

        /// Dry-run
        #[arg(long)]
        dry_run: bool,

        /// Scan sensitive info only
        #[arg(long)]
        scan_sensitive: bool,
    },

    /// Summarize sessions (summary + FTS5 keyword JSON)
    Summarize {
        /// Save summary only
        #[arg(long)]
        summary_only: bool,

        /// Save FTS5 index only
        #[arg(long)]
        fts_only: bool,
    },

    /// Run manage + summarize in one go
    Pipeline {
        /// Skip scan
        #[arg(long)]
        skip_scan: bool,

        /// Skip archive
        #[arg(long)]
        skip_archive: bool,

        /// Skip compact
        #[arg(long)]
        skip_compact: bool,

        /// Skip summarize
        #[arg(long)]
        skip_summarize: bool,

        /// Retention window (days) for archive/compact
        #[arg(short = 'd', long, default_value = "30")]
        days: u64,

        /// Dry-run
        #[arg(long)]
        dry_run: bool,
    },
}

/// CLI 실행
pub fn run(cli: Cli) -> Result<()> {
    let config = build_config(&cli);

    match cli.command {
        Commands::Scan { analyze } => {
            if backend_disabled(&config, Backend::Codex) {
                return Ok(());
            }
            let scanner = CodexScanner::new(config);
            let metas = scanner.scan_all()?;
            scanner.index_sessions(metas)?;

            if analyze {
                scanner.run_analysis()?;
            }
        }

        Commands::Archive { days, dry_run, move_, skip_scan } => {
            if backend_disabled(&config, Backend::Codex) {
                return Ok(());
            }
            // 항상 scan 선행하여 인덱스 최신화 (--skip-scan으로 건너뛰기)
            if !skip_scan {
                let scanner = CodexScanner::new(config.clone());
                let metas = scanner.scan_all()?;
                scanner.index_sessions(metas)?;
            }
            let db = SessionDb::new(&config.codex_index_db)?;
            let archiver = SessionArchiver::new(config);
            // DB 기반 대상 선정 (archived=0, 보존기간 days일보다 오래된 세션)
            let sessions = db.list_archive_candidates(days)?;
            if skip_scan && sessions.is_empty() && db.count_sessions()? == 0 {
                eprintln!("{}", i18n::skip_scan_empty());
            }
            let filtered: Vec<&SessionInfo> = sessions.iter().collect();
            let n = filtered.len();
            if dry_run {
                let mode = if move_ { i18n::mode_delete() } else { i18n::mode_keep() };
                println!("{}", i18n::archive_start_dryrun(n, mode));
            } else if n == 0 {
                println!("{}", i18n::archive_start_none());
            } else if move_ {
                println!("{}", i18n::archive_start_move(n));
            } else {
                println!("{}", i18n::archive_start_keep(n));
            }
            archiver.archive(&filtered, dry_run, move_, &db)?;
        }

        Commands::Restore { session_id, all, days, dry_run, purge } => {
            if backend_disabled(&config, Backend::Codex) {
                return Ok(());
            }
            let db = SessionDb::new(&config.codex_index_db)?;
            let archiver = SessionArchiver::new(config);

            // restore 대상은 DB의 archived 세션 (원본 디렉토리 스캔 의존 제거)
            let rows = if let Some(ref id) = session_id {
                db.list_archived_by_ids(&[id.clone()])?
            } else if all {
                db.list_archived()?
            } else {
                db.list_archived_by_days(days)?
            };

            let n = rows.len();
            if dry_run {
                let mode = if purge { i18n::mode_delete() } else { i18n::mode_keep() };
                println!("{}", i18n::restore_start_dryrun(n, mode));
            } else if n == 0 {
                println!("{}", i18n::restore_start_none());
            } else if purge {
                println!("{}", i18n::restore_start_purge(n));
            } else {
                println!("{}", i18n::restore_start_keep(n));
            }

            archiver.restore(&rows, dry_run, purge, &db)?;
        }

        Commands::List { days, json } => {
            if backend_disabled(&config, Backend::Codex) {
                return Ok(());
            }
            let db = SessionDb::new(&config.codex_index_db)?;
            let archiver = SessionArchiver::new(config);
            let sessions = db.list_sessions_for_display(days)?;
            archiver.list_sessions(&sessions, json)?;
        }

        Commands::Stats { days } => {
            if backend_disabled(&config, Backend::Codex) {
                return Ok(());
            }
            let db = SessionDb::new(&config.codex_index_db)?;
            let archiver = SessionArchiver::new(config);
            let sessions = db.list_sessions_for_display(days)?;
            archiver.show_stats(&sessions)?;
        }

        Commands::Insights { days, top, by, words, json } => {
            if backend_disabled(&config, Backend::Codex) {
                return Ok(());
            }
            crate::insights::run(config, days, top, by, json, words)?;
        }

        Commands::Compact { days, dry_run, scan_sensitive } => {
            if backend_disabled(&config, Backend::Codex) {
                return Ok(());
            }
            let compactor = SessionCompactor::new(config)?;

            if scan_sensitive {
                let sensitive = compactor.discover_sensitive_files()?;
                println!("Found {} files with sensitive data:", sensitive.len());
                for file in &sensitive {
                    println!("  {} ({} bytes, patterns: {:?})",
                        file.path.display(),
                        file.size_bytes,
                        file.patterns
                    );
                }
            } else {
                compactor.compact_sessions(days, dry_run)?;
            }
        }

        Commands::Summarize { summary_only, fts_only } => {
            if backend_disabled(&config, Backend::Hermes) {
                return Ok(());
            }
            let builder = SummaryBuilder::new(config)?;

            if summary_only {
                let (summary_layer, _) = builder.build_summary_layer()?;
                builder.save_summary_layer(&summary_layer)?;
            } else if fts_only {
                let (_, fts_index) = builder.build_summary_layer()?;
                builder.save_fts5_index(&fts_index)?;
            } else {
                builder.run_pipeline()?;
            }
        }

        Commands::Pipeline {
            skip_scan,
            skip_archive,
            skip_compact,
            skip_summarize,
            days,
            dry_run,
        } => {
            // 비활성 백엔드는 해당 단계를 자동으로 건너뜀
            let skip_scan = skip_scan || !config.enabled_codex;
            let skip_archive = skip_archive || !config.enabled_codex;
            let skip_compact = skip_compact || !config.enabled_codex;
            let skip_summarize = skip_summarize || !config.enabled_hermes;

            if !skip_scan {
                println!("Scanning Codex sessions...");
                let scanner = CodexScanner::new(config.clone());
                let metas = scanner.scan_all()?;
                scanner.index_sessions(metas)?;
                println!("Scan complete.\n");
            }

            if !skip_archive && !dry_run {
                println!("Archiving sessions...");
                let db = SessionDb::new(&config.codex_index_db)?;
                let archiver = SessionArchiver::new(config.clone());
                let sessions = db.list_archive_candidates(days)?;
                let filtered: Vec<&SessionInfo> = sessions.iter().collect();
                archiver.archive(&filtered, dry_run, false, &db)?;
                println!("Archive complete.\n");
            }

            if !skip_compact {
                println!("Compacting sessions...");
                let compactor = SessionCompactor::new(config.clone())?;
                compactor.compact_sessions(days, dry_run)?;
                println!("Compact complete.\n");
            }

            if !skip_summarize {
                println!("Summarizing sessions...");
                let builder = SummaryBuilder::new(config)?;
                builder.run_pipeline()?;
                println!("Summarize complete.\n");
            }

            println!("Pipeline complete!");
        }
    }

    Ok(())
}

/// CLI에서 설정 빌드 (파일 → 환경변수 → CLI 플래그 우선순위)
fn build_config(cli: &Cli) -> Config {
    let mut config = Config::load();

    if let Some(ref path) = cli.codex_sessions {
        config.codex_sessions = expand_path(path);
    }
    if let Some(ref path) = cli.hermes_sessions {
        config.hermes_sessions = expand_path(path);
    }
    if let Some(ref path) = cli.codex_archive {
        config.codex_archive = expand_path(path);
    }
    if let Some(ref path) = cli.codex_index_db {
        config.codex_index_db = PathBuf::from(path);
    }
    if let Some(ref path) = cli.summary_layer {
        config.summary_layer = PathBuf::from(path);
    }
    if let Some(ref path) = cli.fts5_index {
        config.fts5_index = PathBuf::from(path);
    }

    // CLI 백엔드 플래그 최종 적용
    if cli.no_codex {
        config.enabled_codex = false;
    }
    if cli.no_hermes {
        config.enabled_hermes = false;
    }

    config
}

/// 백엔드가 비활성이면 경고하고 true 반환 (no-op 처리용)
fn backend_disabled(config: &Config, backend: Backend) -> bool {
    if backend.is_enabled(config) {
        false
    } else {
        let name = match backend {
            Backend::Codex => "codex",
            Backend::Hermes => "hermes",
            Backend::Both => "both",
        };
        eprintln!("{}", i18n::backend_disabled(name));
        true
    }
}

/// 경로 확장 (~ 확장 + 상대경로를 current_dir 기준 절대화)
fn expand_path(path: &str) -> PathBuf {
    let expanded: PathBuf = if path.starts_with("~/") {
        dirs::home_dir().map(|h| h.join(&path[2..])).unwrap_or_else(|| PathBuf::from(path))
    } else {
        PathBuf::from(path)
    };
    if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(&expanded)
    }
}

use std::path::PathBuf;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_path() {
        let home = dirs::home_dir().unwrap();
        let expanded = expand_path("~/test");
        assert_eq!(expanded, home.join("test"));
    }
}
