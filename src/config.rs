//! Session Butler 설정

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// 기본 경로 상수
const DEFAULT_CODEX_SESSIONS: &str = "~/.codex/sessions";
const DEFAULT_CODEX_ARCHIVE: &str = "~/.codex/archive";
const DEFAULT_HERMES_SESSIONS: &str = "~/.hermes/sessions";
const DEFAULT_CODEX_STATE_DB: &str = "~/.codex/state_5.sqlite";
const DEFAULT_CODEX_INDEX_DB: &str = "./codex_index.sqlite";
const DEFAULT_SUMMARY_LAYER: &str = "./summary_layer.json";
const DEFAULT_FTS5_INDEX: &str = "./fts5_index.json";

/// Session Butler 설정
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Codex 세션 디렉토리
    pub codex_sessions: PathBuf,
    /// Codex 압축 저장소
    pub codex_archive: PathBuf,
    /// Hermes 세션 디렉토리
    pub hermes_sessions: PathBuf,
    /// Codex state DB 경로
    pub codex_state_db: PathBuf,
    /// Codex 인덱스 DB 경로
    pub codex_index_db: PathBuf,
    /// Summary layer JSON 경로
    pub summary_layer: PathBuf,
    /// FTS5 인덱스 JSON 경로
    pub fts5_index: PathBuf,
    /// 압축 기본 일수
    pub default_archive_days: u64,
    /// Codex 백엔드 활성화
    pub enabled_codex: bool,
    /// Hermes 백엔드 활성화
    pub enabled_hermes: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

impl Config {
    /// 새 설정 생성
    pub fn new() -> Self {
        Self {
            codex_sessions: expand_path(DEFAULT_CODEX_SESSIONS),
            codex_archive: expand_path(DEFAULT_CODEX_ARCHIVE),
            hermes_sessions: expand_path(DEFAULT_HERMES_SESSIONS),
            codex_state_db: expand_path(DEFAULT_CODEX_STATE_DB),
            codex_index_db: PathBuf::from(DEFAULT_CODEX_INDEX_DB),
            summary_layer: PathBuf::from(DEFAULT_SUMMARY_LAYER),
            fts5_index: PathBuf::from(DEFAULT_FTS5_INDEX),
            default_archive_days: 30,
            enabled_codex: true,
            enabled_hermes: true,
        }
    }

    /// 환경변수를 현재 설정에 적용 (우선순위: default → config 파일 → 환경변수)
    pub fn apply_env(&mut self) {
        if let Ok(path) = std::env::var("CODEX_SESSIONS") {
            self.codex_sessions = expand_path(&path);
        }
        if let Ok(path) = std::env::var("CODEX_ARCHIVE") {
            self.codex_archive = expand_path(&path);
        }
        if let Ok(path) = std::env::var("HERMES_SESSIONS") {
            self.hermes_sessions = expand_path(&path);
        }
        if let Ok(path) = std::env::var("CODEX_STATE_DB") {
            self.codex_state_db = expand_path(&path);
        }
        if let Ok(path) = std::env::var("CODEX_INDEX_DB") {
            self.codex_index_db = PathBuf::from(&path);
        }
        if let Ok(path) = std::env::var("SUMMARY_LAYER_JSON") {
            self.summary_layer = PathBuf::from(&path);
        }
        if let Ok(path) = std::env::var("FTS5_INDEX_JSON") {
            self.fts5_index = PathBuf::from(&path);
        }
        if let Some(b) = parse_bool_env("CODEX_ENABLED") {
            self.enabled_codex = b;
        }
        if let Some(b) = parse_bool_env("HERMES_ENABLED") {
            self.enabled_hermes = b;
        }
    }

    /// 설정 로드 (default → config 파일 → 환경변수).
    /// CLI 플래그(`--no-codex`/`--no-hermes`)는 cli::build_config에서 최종 적용.
    pub fn load() -> Self {
        let mut config = Self::default();

        if let Some(path) = Self::config_file_path() {
            if path.exists() {
                match Self::from_file(&path) {
                    Ok(file_config) => config = file_config,
                    Err(e) => eprintln!(
                        "WARNING: config 파일 로드 실패 (기본값 사용) {}: {}",
                        path.display(), e
                    ),
                }
            }
        }

        config.apply_env();
        config
    }

    /// 기본 config 파일 경로 (플랫폼 무관 ~/.config/session-butler/config.json)
    pub fn config_file_path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".config").join("session-butler").join("config.json"))
    }

    /// 파일에서 설정 로드
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("설정 파일을 읽을 수 없음: {}", path.as_ref().display()))?;

        let mut config: Config = serde_json::from_str(&content)
            .context("설정 파일 파싱 실패")?;

        // 경로 확장 (절대화)
        config.codex_sessions = expand_path(config.codex_sessions.to_string_lossy().as_ref());
        config.codex_archive = expand_path(config.codex_archive.to_string_lossy().as_ref());
        config.hermes_sessions = expand_path(config.hermes_sessions.to_string_lossy().as_ref());
        config.codex_state_db = expand_path(config.codex_state_db.to_string_lossy().as_ref());
        config.codex_index_db = expand_path(config.codex_index_db.to_string_lossy().as_ref());
        config.summary_layer = expand_path(config.summary_layer.to_string_lossy().as_ref());
        config.fts5_index = expand_path(config.fts5_index.to_string_lossy().as_ref());

        Ok(config)
    }

    /// 설정을 파일에 저장
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content = serde_json::to_string_pretty(self)
            .context("설정 직렬화 실패")?;

        std::fs::write(path.as_ref(), content)
            .with_context(|| format!("설정 저장 실패: {}", path.as_ref().display()))?;

        Ok(())
    }

    /// 디렉토리가 존재하는지 확인
    pub fn check_dirs(&self) -> Result<()> {
        if !self.codex_sessions.exists() {
            anyhow::bail!("Codex 세션 디렉토리가 존재하지 않음: {}", self.codex_sessions.display());
        }
        Ok(())
    }
}

/// ~ 확장 + 상대경로를 current_dir 기준 절대경로로 변환.
/// DB에 저장하는 경로를 절대화해 cwd 무관하게 restore/재실행.
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

/// 환경변수를 bool로 파싱. 빈 문자열은 미설정(None) 취급.
/// "0"/"false"/"off"/"no" → false, 그 외 비어있지 않은 값 → true.
fn parse_bool_env(key: &str) -> Option<bool> {
    std::env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            !(v == "0" || v == "false" || v == "off" || v == "no")
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.default_archive_days, 30);
    }

    #[test]
    fn test_expand_path() {
        let path = expand_path("~/test");
        // 홈 디렉토리로 시작해야 함
        assert!(path.to_string_lossy().contains("/"));
    }
}
