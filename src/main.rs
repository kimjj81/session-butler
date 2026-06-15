//! Session Butler - Codex/Hermes 세션 파일 관리 도구
//!
//! TUI 및 CLI 인터페이스 제공

use clap::Parser;
use session_butler::cli::run;
use session_butler::cli::Cli;
use session_butler::config::Config;
use session_butler::tui::run_tui;
use std::process;
use tracing_subscriber;

fn main() {
    // 트레이싱 설정
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args = std::env::args().collect::<Vec<_>>();

    // TUI 모드 (인자 없음 또는 --tui)
    let run_tui_mode = args.len() == 1 || args.iter().any(|a| a == "--tui" || a == "-t");

    if run_tui_mode {
        let config = Config::from_env();
        if let Err(e) = run_tui(config) {
            eprintln!("TUI error: {}", e);
            process::exit(1);
        }
    } else {
        // CLI 모드
        let cli = Cli::parse();
        if let Err(e) = run(cli) {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}
