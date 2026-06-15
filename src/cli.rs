//! CLI 인터페이스

use crate::archive::{SessionArchiver, SessionInfo};
use crate::compact::SessionCompactor;
use crate::config::Config;
use crate::error::Result;
use crate::scanner::CodexScanner;
use crate::summary::SummaryBuilder;
use clap::{Parser, Subcommand};

/// Session Butler - Codex/Hermes 세션 파일 관리 도구
#[derive(Parser, Debug)]
#[command(name = "session-butler")]
#[command(author = "Kim Jeongjin")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Codex/Hermes 세션 파일 관리 도구 (4단계 파이프라인)", long_about = None)]
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
}

/// 세부 명령
#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Phase 1: Codex 세션 스캔 및 인덱싱
    Scan {
        /// 분석 리포트 생성
        #[arg(long)]
        analyze: bool,
    },

    /// Phase 2: 세션 압축 (archive)
    Archive {
        /// 보존 일수 (이전 세션만 대상)
        #[arg(short = 'd', long, default_value = "30")]
        days: u64,

        /// Dry-run (실제 실행 안 함)
        #[arg(long)]
        dry_run: bool,
    },

    /// Phase 2: 세션 복원 (restore)
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
    },

    /// Phase 2: 세션 목록
    List {
        /// 대상 일수
        #[arg(short = 'd', long, default_value = "30")]
        days: u64,

        /// JSON 출력
        #[arg(long)]
        json: bool,
    },

    /// Phase 2: 통계
    Stats {
        /// 대상 일수
        #[arg(short = 'd', long, default_value = "30")]
        days: u64,
    },

    /// Phase 3: Compaction
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

    /// Phase 4: Hermes 세션 요약
    Summarize {
        /// 요약만 저장
        #[arg(long)]
        summary_only: bool,

        /// FTS5 인덱스만 저장
        #[arg(long)]
        fts_only: bool,
    },

    /// 전체 파이프라인 실행
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
            let scanner = CodexScanner::new(config);
            let metas = scanner.scan_all()?;
            scanner.index_sessions(metas)?;

            if analyze {
                scanner.run_analysis()?;
            }
        }

        Commands::Archive { days, dry_run } => {
            let archiver = SessionArchiver::new(config);
            let sessions = archiver.discover_sessions()?;
            let filtered = archiver.filter_by_days(&sessions, days);
            archiver.archive(&filtered, dry_run)?;
        }

        Commands::Restore { session_id, all, days, dry_run } => {
            let archiver = SessionArchiver::new(config);
            let sessions = archiver.discover_sessions()?;

            let filtered: Vec<&SessionInfo> = if let Some(ref id) = session_id {
                sessions.iter()
                    .filter(|s| s.session_id.as_deref() == Some(id))
                    .collect()
            } else if all {
                sessions.iter().collect()
            } else {
                archiver.filter_by_days(&sessions, days)
            };

            archiver.restore(&filtered, dry_run)?;
        }

        Commands::List { days, json } => {
            let archiver = SessionArchiver::new(config);
            let sessions = archiver.discover_sessions()?;
            let filtered_refs = archiver.filter_by_days(&sessions, days);
            let filtered: Vec<_> = filtered_refs.into_iter().cloned().collect();
            archiver.list_sessions(&filtered, json)?;
        }

        Commands::Stats { days } => {
            let archiver = SessionArchiver::new(config);
            let sessions = archiver.discover_sessions()?;
            let filtered_refs = archiver.filter_by_days(&sessions, days);
            let filtered: Vec<_> = filtered_refs.into_iter().cloned().collect();
            archiver.show_stats(&filtered)?;
        }

        Commands::Compact { days, dry_run, scan_sensitive } => {
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
            if !skip_scan {
                println!("Phase 1: Scanning Codex sessions...");
                let scanner = CodexScanner::new(config.clone());
                let metas = scanner.scan_all()?;
                scanner.index_sessions(metas)?;
                println!("Phase 1 complete.\n");
            }

            if !skip_archive && !dry_run {
                println!("Phase 2: Archiving sessions...");
                let archiver = SessionArchiver::new(config.clone());
                let sessions = archiver.discover_sessions()?;
                let filtered = archiver.filter_by_days(&sessions, days);
                archiver.archive(&filtered, dry_run)?;
                println!("Phase 2 complete.\n");
            }

            if !skip_compact {
                println!("Phase 3: Compacting sessions...");
                let compactor = SessionCompactor::new(config.clone())?;
                compactor.compact_sessions(days, dry_run)?;
                println!("Phase 3 complete.\n");
            }

            if !skip_summarize {
                println!("Phase 4: Summarizing Hermes sessions...");
                let builder = SummaryBuilder::new(config)?;
                builder.run_pipeline()?;
                println!("Phase 4 complete.\n");
            }

            println!("Pipeline complete!");
        }
    }

    Ok(())
}

/// CLI에서 설정 빌드
fn build_config(cli: &Cli) -> Config {
    let mut config = Config::from_env();

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

    config
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
