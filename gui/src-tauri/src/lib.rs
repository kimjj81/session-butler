// Tauri 커맨드 — session-butler 라이브러리 호출.
// 읽기(insights)는 데이터 반환, 긴 작업(scan)은 spawn_blocking + 진행률 이벤트.

use session_butler::config::Config;
use session_butler::insights::{self, Granularity, Report, WordsSource};
use session_butler::progress::{Bar, Progress};
use session_butler::scanner::CodexScanner;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

/// DB 경로 해석: 기본 상대경로(codex_index.sqlite)가 그대로면 dev 편의상
/// CWD / 상위 / $HOME 에서 기존 인덱스를 찾아 절대경로로 보정한다.
fn resolve_db(mut config: Config) -> Config {
    let p = &config.codex_index_db;
    if !p.exists() && !p.is_absolute() {
        let name = p
            .file_name()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("codex_index.sqlite"));
        let cwd = std::env::current_dir().unwrap_or_default();
        let home = std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        for c in [cwd.join(&name), cwd.join("..").join(&name), home.join(&name)] {
            if c.exists() {
                config.codex_index_db = c;
                break;
            }
        }
    }
    config
}

fn load_config() -> Config {
    resolve_db(Config::load())
}

fn parse_granularity(s: &str) -> Granularity {
    match s {
        "day" => Granularity::Day,
        "week" => Granularity::Week,
        _ => Granularity::Month,
    }
}

fn parse_words(s: &str) -> WordsSource {
    match s {
        "conversation" => WordsSource::Conversation,
        "reasoning" => WordsSource::Reasoning,
        "tools" => WordsSource::Tools,
        "first-prompt" => WordsSource::FirstPrompt,
        _ => WordsSource::All,
    }
}

#[derive(Serialize)]
struct ScanSummary {
    sessions: usize,
}

/// 진행률 → Tauri 이벤트("scan-progress")로 송출하는 Progress 구현체.
struct EventProgress {
    app: AppHandle,
}

impl Progress for EventProgress {
    fn bar(&self, len: u64, msg: &str) -> Box<dyn Bar> {
        let _ = self.app.emit(
            "scan-progress",
            serde_json::json!({ "kind": "bar", "len": len, "msg": msg }),
        );
        Box::new(EventBar {
            app: self.app.clone(),
            len,
        })
    }

    fn spinner(&self, msg: &str) -> Box<dyn Bar> {
        let _ = self.app.emit(
            "scan-progress",
            serde_json::json!({ "kind": "spinner", "msg": msg }),
        );
        Box::new(EventBar {
            app: self.app.clone(),
            len: 0,
        })
    }
}

struct EventBar {
    app: AppHandle,
    len: u64,
}

impl Bar for EventBar {
    fn inc(&self, n: u64) {
        let _ = self.app.emit(
            "scan-progress",
            serde_json::json!({ "kind": "inc", "n": n, "len": self.len }),
        );
    }

    fn finish(&self) {
        let _ = self.app.emit("scan-progress", serde_json::json!({ "kind": "finish" }));
    }
}

/// 인사이트 리포트 반환 (데이터). 세션이 없으면 null.
#[tauri::command]
async fn get_insights(days: u64, top: usize, by: String, words: String) -> Result<Option<Report>, String> {
    let config = load_config();
    let by = parse_granularity(&by);
    let words = parse_words(&words);
    tauri::async_runtime::spawn_blocking(move || {
        insights::build_report(&config, days, top, by, words).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 세션 스캔 + 인덱싱. 진행률은 "scan-progress" 이벤트로 송출.
#[tauri::command]
async fn scan(app: AppHandle) -> Result<ScanSummary, String> {
    let config = load_config();
    let progress: Arc<dyn Progress> = Arc::new(EventProgress { app });
    tauri::async_runtime::spawn_blocking(move || {
        let scanner = CodexScanner::new(config).with_progress(progress);
        let metas = scanner.scan_all().map_err(|e| e.to_string())?;
        let n = metas.len();
        scanner.index_sessions(metas).map_err(|e| e.to_string())?;
        Ok::<ScanSummary, String>(ScanSummary { sessions: n })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![get_insights, scan])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
