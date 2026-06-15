//! Session Butler - Codex/Hermes 세션 파일 관리 도구
//!
//! TUI 및 CLI 인터페이스 제공

use clap::Parser;
use session_butler::cli::run;
use session_butler::cli::Cli;
use session_butler::config::Config;
use session_butler::i18n;
use session_butler::tui::run_tui;
use std::io::{self, Write};
use std::process;
use tracing_subscriber;

fn main() {
    // 트레이싱 설정
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    // 첫 실행: config 파일이 없으면 언어 선택(LANG 감지 + 확인) → config 저장
    if !Config::config_exists() {
        let lang = choose_first_run_language();
        let mut config = Config::default();
        config.language = lang;
        if let Some(path) = Config::config_file_path() {
            let _ = config.save(&path);
        }
    }

    let config = Config::load();
    i18n::set_lang(&config.language);

    let args = std::env::args().collect::<Vec<_>>();

    // TUI 모드 (인자 없음 또는 --tui)
    let run_tui_mode = args.len() == 1 || args.iter().any(|a| a == "--tui" || a == "-t");

    if run_tui_mode {
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

/// 첫 실행 언어 선택: LANG/LC_ALL 감지 후 확인 프롬프트. 언어 코드("ko"/"en") 반환.
fn choose_first_run_language() -> String {
    let detected_ko = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LANG"))
        .map(|v| v.to_ascii_lowercase().starts_with("ko"))
        .unwrap_or(false);

    // 감지된 언어로 프롬프트 표시
    i18n::set_lang(if detected_ko { "ko" } else { "en" });
    print!("{}", i18n::first_run_prompt(detected_ko));
    let _ = io::stdout().flush();

    let mut line = String::new();
    let _ = io::stdin().read_line(&mut line);
    let answer = line.trim().to_ascii_lowercase();

    // 빈 입력 = 감지값 수락(Y)
    let yes = answer.is_empty()
        || answer.starts_with('y')
        || answer == "yes"
        || answer == "예"
        || answer == "네";

    if detected_ko {
        if yes { "ko".to_string() } else { "en".to_string() }
    } else {
        if yes { "en".to_string() } else { "ko".to_string() }
    }
}
