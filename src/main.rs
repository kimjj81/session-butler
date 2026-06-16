//! Session Butler - 세션 로그 관리 도구
//!
//! TUI 및 CLI 인터페이스 제공

use clap::Parser;
use session_butler::cli::run;
use session_butler::cli::Cli;
use session_butler::config::Config;
use session_butler::i18n;
use session_butler::tui::run_tui;
use std::io::{self, IsTerminal, Write};
use std::process;
use tracing_subscriber;

fn main() {
    // 트레이싱 설정
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    let args = std::env::args().collect::<Vec<_>>();

    // config 로드 (파일이 없으면 기본값)
    let mut config = Config::load();

    // 첫 실행(config 파일 부재): 언어 선택.
    // 단, 비대화형(stdin이 TTY가 아님)이거나 --help/-h/--version/-V/help 이면
    // 프롬프트를 생략해 CLI/help 정상 동작을 보존한다.
    if !Config::config_exists() {
        let interactive = should_prompt_first_run(&args);
        config.language = choose_first_run_language(interactive);
        if interactive {
            // 영속화 (save는 부모 디렉토리 생성 포함). 실패 시 경고하고 계속.
            if let Some(path) = Config::config_file_path() {
                if let Err(e) = config.save(&path) {
                    eprintln!("WARNING: 설정 저장 실패 (다음 실행에 재시도): {}", e);
                }
            }
        }
        // 비대화형/help: 감지값을 이번 실행에만 사용 (영속화 생략 → 다음 대화형 실행에 프롬프트)
    }

    i18n::set_lang(&config.language);

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

/// 첫 실행 프롬프트를 띄워도 되는지: stdin이 TTY이고 help/version 계열 플래그가 없을 때.
/// 스크립트/파이프 환경이나 --help/-h/--version/-V/help 호출에서는 프롬프트 없이 감지만 수행.
fn should_prompt_first_run(args: &[String]) -> bool {
    if !io::stdin().is_terminal() {
        return false;
    }
    !args
        .iter()
        .any(|a| matches!(a.as_str(), "--help" | "-h" | "--version" | "-V" | "help"))
}

/// 첫 실행 언어 선택. `prompt=true`면 LANG/LC_ALL 감지 후 확인 프롬프트,
/// `false`면 감지값만 반환(프롬프트/영속화 없음). 언어 코드("ko"/"en") 반환.
fn choose_first_run_language(prompt: bool) -> String {
    let detected_ko = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LANG"))
        .map(|v| v.to_ascii_lowercase().starts_with("ko"))
        .unwrap_or(false);

    if !prompt {
        // 비대화형/help: 감지값만 사용 (영속화하지 않으므로 다음 실행에 재확인)
        return if detected_ko { "ko".to_string() } else { "en".to_string() };
    }

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
