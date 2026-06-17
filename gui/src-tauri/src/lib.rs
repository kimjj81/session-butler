// Tauri 커맨드 — session-butler 라이브러리 호출.
// 읽기(insights)는 데이터 반환, 긴 작업(scan)은 spawn_blocking + 진행률 이벤트.

use session_butler::archive::SessionArchiver;
use session_butler::compact::SessionCompactor;
use session_butler::config::Config;
use session_butler::db::SessionDb;
use session_butler::insights::{self, Granularity, Report, WordsSource};
use session_butler::progress::{Bar, Progress};
use session_butler::scanner::CodexScanner;
use session_butler::summary::SummaryBuilder;
use session_butler::types::{ArchivedSessionRow, SessionInfo, SensitiveFile};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

/// 쓰기 가능한 출력(DB/summary/fts5)을 저장소 밖 app-data 로 강제 재배치.
/// 이유 — Config::load()/from_file 이 상대경로를 CWD 기준 절대화하는데, `tauri dev`
/// 의 CWD 는 `gui/src-tauri`(파일 감시 대상)라 그 안 파일이 바뀌면 앱 재빌드 루프가
/// 발생. is_absolute() 검사로는 Config 의 CWD-절대경로를 못 잡아 **무조건** 재배치.
/// CODEX_INDEX_DB 환경변수가 있으면 DB 경로만 그 값으로(테스트/커스텀용).
fn relocate_outputs(mut config: Config) -> Config {
    let dir = app_data_dir();
    let _ = std::fs::create_dir_all(&dir);
    match std::env::var("CODEX_INDEX_DB") {
        Ok(p) if !p.trim().is_empty() => {
            config.codex_index_db = std::path::PathBuf::from(p);
        }
        _ => {
            config.codex_index_db = dir.join("index.sqlite");
        }
    }
    config.summary_layer = dir.join("summary_layer.json");
    config.fts5_index = dir.join("fts5_index.json");
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
    relocate_outputs(Config::load())
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

/// 진행률 → Tauri 이벤트로 송출하는 Progress 구현체. `event` 로 커맨드별 채널 분리.
struct EventProgress {
    app: AppHandle,
    event: &'static str,
}

impl Progress for EventProgress {
    fn bar(&self, len: u64, msg: &str) -> Box<dyn Bar> {
        let _ = self.app.emit(
            self.event,
            serde_json::json!({ "kind": "bar", "len": len, "msg": msg }),
        );
        Box::new(EventBar {
            app: self.app.clone(),
            event: self.event,
            len,
        })
    }

    fn spinner(&self, msg: &str) -> Box<dyn Bar> {
        let _ = self.app.emit(
            self.event,
            serde_json::json!({ "kind": "spinner", "msg": msg }),
        );
        Box::new(EventBar {
            app: self.app.clone(),
            event: self.event,
            len: 0,
        })
    }
}

struct EventBar {
    app: AppHandle,
    event: &'static str,
    len: u64,
}

impl Bar for EventBar {
    fn inc(&self, n: u64) {
        let _ = self.app.emit(
            self.event,
            serde_json::json!({ "kind": "inc", "n": n, "len": self.len }),
        );
    }

    fn finish(&self) {
        let _ = self.app.emit(self.event, serde_json::json!({ "kind": "finish" }));
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
    let progress: Arc<dyn Progress> = Arc::new(EventProgress { app, event: "scan-progress" });
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

#[derive(Serialize)]
struct ArchiveSummary {
    archived: usize,
    skipped: usize,
    total_original: u64,
    total_compressed: u64,
}

/// 보존기간(days)보다 오래된 세션을 zstd 압축. 진행률: "archive-progress".
#[tauri::command]
async fn archive(app: AppHandle, days: u64, dry_run: bool, move_originals: bool) -> Result<ArchiveSummary, String> {
    let config = load_config();
    let progress: Arc<dyn Progress> = Arc::new(EventProgress { app, event: "archive-progress" });
    tauri::async_runtime::spawn_blocking(move || {
        let db = SessionDb::new(&config.codex_index_db).map_err(|e| e.to_string())?;
        // 파괴적 archive 전에 DB를 디스크와 동기화(scan). app-data DB 가 비어있거나
        // 오래된 상태에서 move_originals=true 가 되면, mark_archived 실패 후에도
        // 원본이 이미 삭제돼 복구 불가 상태가 되는 데이터 손실(Codex 리뷰 P1)을 막는다.
        let scanner = CodexScanner::new(config.clone()).with_progress(progress.clone());
        let metas = scanner.scan_all().map_err(|e| e.to_string())?;
        scanner.index_sessions(metas).map_err(|e| e.to_string())?;
        let archiver = SessionArchiver::new(config).with_progress(progress);
        // DB 기반 보관 대상 선정: date < cutoff 이고 미보관(archived=0).
        // (filter_by_days 는 조회용 '최근 N일' 필터라 방향이 반대 — Codex 리뷰 P1.)
        let candidates = db.list_archive_candidates(days).map_err(|e| e.to_string())?;
        let old: Vec<&SessionInfo> = candidates.iter().collect();
        let res = archiver.archive(&old, dry_run, move_originals, &db).map_err(|e| e.to_string())?;
        Ok::<ArchiveSummary, String>(ArchiveSummary {
            archived: res.archived.len(),
            skipped: res.skipped.len(),
            total_original: res.total_original,
            total_compressed: res.total_compressed,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 보관된(archived) 세션 목록(restore 표시용).
#[tauri::command]
async fn list_archived() -> Result<Vec<ArchivedSessionRow>, String> {
    let config = load_config();
    tauri::async_runtime::spawn_blocking(move || {
        let db = SessionDb::new(&config.codex_index_db).map_err(|e| e.to_string())?;
        db.list_archived().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
struct RestoreSummary {
    restored: usize,
}

/// 보관 세션 전체 복원. 진행률: "restore-progress".
#[tauri::command]
async fn restore(app: AppHandle, dry_run: bool, purge: bool) -> Result<RestoreSummary, String> {
    let config = load_config();
    let progress: Arc<dyn Progress> = Arc::new(EventProgress { app, event: "restore-progress" });
    tauri::async_runtime::spawn_blocking(move || {
        let db = SessionDb::new(&config.codex_index_db).map_err(|e| e.to_string())?;
        let rows = db.list_archived().map_err(|e| e.to_string())?;
        let archiver = SessionArchiver::new(config).with_progress(progress);
        let restored = archiver.restore(&rows, dry_run, purge, &db).map_err(|e| e.to_string())?;
        Ok::<RestoreSummary, String>(RestoreSummary { restored: restored.len() })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
struct CompactSummary {
    moved: usize,
    skipped: usize,
    total: usize,
}

/// 오래된 세션 compaction(trash 이동). 진행률: "compact-progress".
#[tauri::command]
async fn compact(app: AppHandle, days: u64, dry_run: bool) -> Result<CompactSummary, String> {
    let config = load_config();
    let progress: Arc<dyn Progress> = Arc::new(EventProgress { app, event: "compact-progress" });
    tauri::async_runtime::spawn_blocking(move || {
        let compactor = SessionCompactor::new(config).map_err(|e| e.to_string())?.with_progress(progress);
        let res = compactor.compact_sessions(days, dry_run).map_err(|e| e.to_string())?;
        Ok::<CompactSummary, String>(CompactSummary {
            moved: res.moved.len(),
            skipped: res.skipped.len(),
            total: res.total,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 민감정보(.env/token/key) 포함 세션 탐지. 진행률: "scan-sensitive-progress".
#[tauri::command]
async fn scan_sensitive(app: AppHandle) -> Result<Vec<SensitiveFile>, String> {
    let config = load_config();
    let progress: Arc<dyn Progress> = Arc::new(EventProgress { app, event: "scan-sensitive-progress" });
    tauri::async_runtime::spawn_blocking(move || {
        let compactor = SessionCompactor::new(config).map_err(|e| e.to_string())?.with_progress(progress);
        compactor.discover_sensitive_files().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Hermes/session_*.json 요약 + FTS5 인덱스 생성. summary_only/fts_only 로 부분 실행.
#[tauri::command]
async fn summarize(summary_only: bool, fts_only: bool) -> Result<(), String> {
    let config = load_config();
    // CLI 와 동일: Hermes 백엔드 비활성 시 실행하지 않는다(Codex 리뷰 P2).
    if !config.enabled_hermes {
        return Err("summary 백엔드가 비활성 — Settings에서 Hermes 수집을 켜세요.".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let builder = SummaryBuilder::new(config).map_err(|e| e.to_string())?;
        if summary_only {
            let (sl, _) = builder.build_summary_layer().map_err(|e| e.to_string())?;
            builder.save_summary_layer(&sl).map_err(|e| e.to_string())?;
        } else if fts_only {
            let (_, fts) = builder.build_summary_layer().map_err(|e| e.to_string())?;
            builder.save_fts5_index(&fts).map_err(|e| e.to_string())?;
        } else {
            builder.run_pipeline().map_err(|e| e.to_string())?;
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 설정 조회(GUI 에서 편집 가능한 항목).
#[derive(Serialize)]
struct ConfigView {
    codex_sessions: String,
    hermes_sessions: String,
    default_archive_days: u64,
    enabled_codex: bool,
    enabled_hermes: bool,
    language: String,
}

#[tauri::command]
async fn get_config() -> Result<ConfigView, String> {
    let c = load_config();
    Ok(ConfigView {
        codex_sessions: c.codex_sessions.display().to_string(),
        hermes_sessions: c.hermes_sessions.display().to_string(),
        default_archive_days: c.default_archive_days,
        enabled_codex: c.enabled_codex,
        enabled_hermes: c.enabled_hermes,
        language: c.language,
    })
}

/// 설정 부분 갱신 후 config.json 에 저장. None 필드는 미변경.
#[tauri::command]
async fn set_config(
    codex_sessions: Option<String>,
    default_archive_days: Option<u64>,
    enabled_codex: Option<bool>,
    enabled_hermes: Option<bool>,
    language: Option<String>,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut c = Config::load();
        if let Some(p) = codex_sessions {
            let p = p.trim();
            if !p.is_empty() {
                c.codex_sessions = std::path::PathBuf::from(p);
            }
        }
        if let Some(d) = default_archive_days {
            c.default_archive_days = d;
        }
        if let Some(b) = enabled_codex {
            c.enabled_codex = b;
        }
        if let Some(b) = enabled_hermes {
            c.enabled_hermes = b;
        }
        if let Some(l) = language {
            c.language = l;
        }
        if let Some(path) = Config::config_file_path() {
            c.save(&path).map_err(|e| e.to_string())?;
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_insights,
            scan,
            archive,
            list_archived,
            restore,
            compact,
            scan_sensitive,
            summarize,
            get_config,
            set_config
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Config::load() 가 config.json 유무와 무관하게 CWD-절대경로를 만들어도
    /// GUI는 항상 app-data(저장소 밖)로 재배치해야 한다 — tauri dev 감시 루프 방지.
    #[test]
    fn relocate_outputs_always_outside_repo() {
        // config.json 로드 후 expand_path 가 만드는 것과 같은 CWD 기반 절대경로 시뮬레이션
        let cwd = std::env::current_dir().unwrap_or_default();
        let mut cfg = Config::new();
        cfg.codex_index_db = cwd.join("./codex_index.sqlite");
        assert!(cfg.codex_index_db.is_absolute(), "전제: Config가 절대경로로 만듭");

        let resolved = relocate_outputs(cfg);
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
