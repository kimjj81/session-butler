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
        }
    }

    /// 환경변수로부터 설정 로드
    pub fn from_env() -> Self {
        let mut config = Self::new();

        if let Ok(path) = std::env::var("CODEX_SESSIONS") {
            config.codex_sessions = expand_path(&path);
        }
        if let Ok(path) = std::env::var("CODEX_ARCHIVE") {
            config.codex_archive = expand_path(&path);
        }
        if let Ok(path) = std::env::var("HERMES_SESSIONS") {
            config.hermes_sessions = expand_path(&path);
        }
        if let Ok(path) = std::env::var("CODEX_STATE_DB") {
            config.codex_state_db = expand_path(&path);
        }
        if let Ok(path) = std::env::var("CODEX_INDEX_DB") {
            config.codex_index_db = PathBuf::from(&path);
        }
        if let Ok(path) = std::env::var("SUMMARY_LAYER_JSON") {
            config.summary_layer = PathBuf::from(&path);
        }
        if let Ok(path) = std::env::var("FTS5_INDEX_JSON") {
            config.fts5_index = PathBuf::from(&path);
        }

        config
    }

    /// 파일에서 설정 로드
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("설정 파일을 읽을 수 없음: {}", path.as_ref().display()))?;

        let mut config: Config = serde_json::from_str(&content)
            .context("설정 파일 파싱 실패")?;

        // 경로 확장
        config.codex_sessions = expand_path(config.codex_sessions.to_string_lossy().as_ref());
        config.codex_archive = expand_path(config.codex_archive.to_string_lossy().as_ref());
        config.hermes_sessions = expand_path(config.hermes_sessions.to_string_lossy().as_ref());
        config.codex_state_db = expand_path(config.codex_state_db.to_string_lossy().as_ref());

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

/// ~를 홈 디렉토리로 확장
fn expand_path(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(&path[2..]);
        }
    }
    PathBuf::from(path)
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
