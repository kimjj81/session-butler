//! CLI 인터페이스

use crate::archive::SessionArchiver;
use crate::compact::SessionCompactor;
use crate::config::Config;
use crate::db::SessionDb;
use crate::error::Result;
use crate::scanner::CodexScanner;
use crate::summary::SummaryBuilder;
use crate::types::SessionInfo;
use clap::{Parser, Subcommand};

/// Session Butler - Codex/Hermes 세션 파일 관리 도구
#[derive(Parser, Debug)]
#[command(name = "session-butler")]
#[command(author = "Kim Jeongjin")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(
    about = "Codex/Hermes 세션 기록을 압축·보관·요약하는 도구",
    long_about = "Codex 세션(rollout-*.jsonl)은 스캔·압축·복원·컴팩션으로 관리하고, \
Hermes 세션(session_*.json)은 요약·키워드화해 검색 가능한 지식베이스로 만듭니다.\n\
백엔드 활성화 우선순위: config 파일 → 환경변수(CODEX_ENABLED/HERMES_ENABLED) → \
--no-codex/--no-hermes"
)]
pub struct Cli {
    /// 명령
    #[command(subcommand)]
    pub command: Commands,

    /// Codex 세션 디렉토리
    #[arg(short = 'C', long, global = true)]
    pub codex_sessions: Option<String>,

    /// Hermes 세션 디렉토리
    #[arg(short = 'H', long, global = true)]
    pub hermes_sessions: Option<String>,

    /// Codex 아카이브 디렉토리
    #[arg(short = 'A', long, global = true)]
    pub codex_archive: Option<String>,

    /// Codex 인덱스 DB 경로
    #[arg(short = 'I', long, global = true)]
    pub codex_index_db: Option<String>,

    /// 요약 레이어 JSON 경로
    #[arg(short = 'S', long, global = true)]
    pub summary_layer: Option<String>,

    /// FTS5 인덱스 JSON 경로
    #[arg(short = 'F', long, global = true)]
    pub fts5_index: Option<String>,

    /// 상세 출력
    #[arg(short = 'v', long, global = true)]
    pub verbose: bool,

    /// Codex 백엔드 비활성 (scan/archive/restore/list/stats/compact 건너뜀)
    #[arg(long, global = true)]
    pub no_codex: bool,

    /// Hermes 백엔드 비활성 (summarize 건너뜀)
    #[arg(long, global = true)]
    pub no_hermes: bool,
}

/// 세부 명령
#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Codex: 세션 스캔 및 인덱싱 (FTS5 전문검색 인덱스 구축)
    Scan {
        /// 분석 리포트 생성
        #[arg(long)]
        analyze: bool,
    },

    /// Codex: 세션 압축 (--move 원본 삭제, --skip-scan 사전 스캔 생략)
    Archive {
        /// 보존 일수 (이전 세션만 대상)
        #[arg(short = 'd', long, default_value = "30")]
        days: u64,

        /// Dry-run (실제 실행 안 함)
        #[arg(long)]
        dry_run: bool,

        /// 압축 후 원본 .jsonl 삭제 (이동)
        #[arg(long = "move")]
        move_: bool,

        /// archive 전 scan(인덱스 최신화) 건너뛰기
        #[arg(long)]
        skip_scan: bool,
    },

    /// Codex: 세션 복원 (DB의 archived 세션, --purge 보관본까지 삭제)
    Restore {
        /// 복원할 세션 ID
        #[arg(long)]
        session_id: Option<String>,

        /// 전체 복원
        #[arg(long)]
        all: bool,

        /// 대상 일수
        #[arg(short = 'd', long, default_value = "30")]
        days: u64,

        /// Dry-run
        #[arg(long)]
        dry_run: bool,

        /// 복원 후 보관본(.zst) 삭제 + archived 해제
        #[arg(long)]
        purge: bool,
    },

    /// Codex: 세션 목록
    List {
        /// 대상 일수
        #[arg(short = 'd', long, default_value = "30")]
        days: u64,

        /// JSON 출력
        #[arg(long)]
        json: bool,
    },

    /// Codex: 통계
    Stats {
        /// 대상 일수
        #[arg(short = 'd', long, default_value = "30")]
        days: u64,
    },

    /// Codex: 컴팩션 + 민감정보(.env/token/key) 탐지
    Compact {
        /// 대상 일수
        #[arg(short = 'd', long, default_value = "0")]
        days: u64,

        /// Dry-run
        #[arg(long)]
        dry_run: bool,

        /// 민감정보 스캔만 수행
        #[arg(long)]
        scan_sensitive: bool,
    },

    /// Hermes: 세션 요약 (요약 + FTS5 키워드 JSON)
    Summarize {
        /// 요약만 저장
        #[arg(long)]
        summary_only: bool,

        /// FTS5 인덱스만 저장
        #[arg(long)]
        fts_only: bool,
    },

    /// Codex+Hermes: 관리·요약을 한 번에 실행
    Pipeline {
        /// Phase 1 건너뜀
        #[arg(long)]
        skip_scan: bool,

        /// Phase 2 건너뜀
        #[arg(long)]
        skip_archive: bool,

        /// Phase 3 건너뜀
        #[arg(long)]
        skip_compact: bool,

        /// Phase 4 건너뜀
        #[arg(long)]
        skip_summarize: bool,

        /// 보존 일수
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
            if backend_disabled(&config, "codex") {
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
            if backend_disabled(&config, "codex") {
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
            // DB 기반 대상 선정 (archived=0, 최근 days일)
            let sessions = db.list_active_by_days(days)?;
            let filtered: Vec<&SessionInfo> = sessions.iter().collect();
            archiver.archive(&filtered, dry_run, move_, &db)?;
        }

        Commands::Restore { session_id, all, days, dry_run, purge } => {
            if backend_disabled(&config, "codex") {
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

            archiver.restore(&rows, dry_run, purge, &db)?;
        }

        Commands::List { days, json } => {
            if backend_disabled(&config, "codex") {
                return Ok(());
            }
            let archiver = SessionArchiver::new(config);
            let sessions = archiver.discover_sessions()?;
            let filtered_refs = archiver.filter_by_days(&sessions, days);
            let filtered: Vec<_> = filtered_refs.into_iter().cloned().collect();
            archiver.list_sessions(&filtered, json)?;
        }

        Commands::Stats { days } => {
            if backend_disabled(&config, "codex") {
                return Ok(());
            }
            let archiver = SessionArchiver::new(config);
            let sessions = archiver.discover_sessions()?;
            let filtered_refs = archiver.filter_by_days(&sessions, days);
            let filtered: Vec<_> = filtered_refs.into_iter().cloned().collect();
            archiver.show_stats(&filtered)?;
        }

        Commands::Compact { days, dry_run, scan_sensitive } => {
            if backend_disabled(&config, "codex") {
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
            if backend_disabled(&config, "hermes") {
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
                let sessions = db.list_active_by_days(days)?;
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
                println!("Summarizing Hermes sessions...");
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
fn backend_disabled(config: &Config, backend: &str) -> bool {
    let disabled = match backend {
        "codex" => !config.enabled_codex,
        "hermes" => !config.enabled_hermes,
        _ => false,
    };
    if disabled {
        eprintln!("{} 백엔드 비활성 — 건너뜁니다 (--no-{} 또는 CODEX/HERMES_ENABLED 확인)", backend, backend);
    }
    disabled
}

/// 경로 확장 (~를 홈 디렉토리로)
fn expand_path(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(&path[2..]);
        }
    }
    PathBuf::from(path)
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
