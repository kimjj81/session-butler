// Tauri 커맨드 — session-butler 라이브러리 호출.
// 읽기(insights)는 데이터 반환, 긴 작업(scan)은 spawn_blocking + 진행률 이벤트.

use session_butler::config::Config;
use session_butler::insights::{self, Granularity, Report, WordsSource};
use session_butler::progress::{Bar, Progress};
use session_butler::scanner::CodexScanner;
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

/// DB 경로 해석: GUI는 Config 경로와 무관하게 **항상 저장소 밖(app-data)의 자체
/// DB**를 쓴다. 이유 — Config::load()/from_file 이 상대경로를 CWD 기준 절대화하는데,
/// `tauri dev` 의 CWD는 `gui/src-tauri`(파일 감시 대상)라 그 안의 SQLite WAL 파일
/// (-shm/-wal)이 변경되면 앱 재빌드 무한 루프를 일으킨다. 명시적 CODEX_INDEX_DB
/// 환경변수가 있을 때만 그 값을 그대로 쓴다(테스트/커스텀 경로용).
fn resolve_db(mut config: Config) -> Config {
    if let Ok(p) = std::env::var("CODEX_INDEX_DB") {
        if !p.trim().is_empty() {
            config.codex_index_db = std::path::PathBuf::from(p);
            return config;
        }
    }
    let dir = app_data_dir();
    let _ = std::fs::create_dir_all(&dir);
    config.codex_index_db = dir.join("index.sqlite");
    config
}

/// 앱 데이터 디렉토리(저장소 밖). mac: ~/Library/Application Support/session-butler
fn app_data_dir() -> std::path::PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        return std::path::PathBuf::from(xdg).join("session-butler");
    }
    let sub: &str = match std::env::consts::OS {
        "macos" => "Library/Application Support/session-butler",
        "windows" => "AppData/Roaming/session-butler",
        _ => ".local/share/session-butler",
    };
    std::env::var("HOME")
        .map(|h| std::path::PathBuf::from(h).join(sub))
        .unwrap_or_else(|_| std::env::temp_dir().join("session-butler"))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Config::load() 가 config.json 유무와 무관하게 CWD-절대경로를 만들어도
    /// GUI는 항상 app-data(저장소 밖)로 재배치해야 한다 — tauri dev 감시 루프 방지.
    #[test]
    fn resolve_db_always_relocates_outside_repo() {
        // config.json 로드 후 expand_path 가 만드는 것과 같은 CWD 기반 절대경로 시뮬레이션
        let cwd = std::env::current_dir().unwrap_or_default();
        let mut cfg = Config::new();
        cfg.codex_index_db = cwd.join("./codex_index.sqlite");
        assert!(cfg.codex_index_db.is_absolute(), "전제: Config가 절대경로로 만듦");

        let resolved = resolve_db(cfg);
        assert!(
            resolved.codex_index_db.is_absolute(),
            "절대경로여야 함: {:?}",
            resolved.codex_index_db
        );
        assert!(
            !resolved.codex_index_db.starts_with(&cwd),
            "CWD(감시 대상 src-tauri 포함) 밖이어야 함: {:?}",
            resolved.codex_index_db
        );
        assert!(
            resolved.codex_index_db.ends_with("index.sqlite"),
            "app-data index.sqlite 여야 함: {:?}",
            resolved.codex_index_db
        );
    }
}
