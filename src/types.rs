//! Session Butler 공통 타입 정의

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// 세션 메타데이터 (Codex)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSessionMeta {
    pub path: PathBuf,
    pub filename: String,
    pub session_id: String,
    pub date: Option<String>,
    pub cwd: Option<String>,
    pub first_user_prompt: Option<String>,
    pub model_provider: Option<String>,
    pub cli_version: Option<String>,
    pub source: Option<String>,
    pub model: Option<String>,
    pub git_sha: Option<String>,
    pub git_branch: Option<String>,
    pub git_origin_url: Option<String>,
    pub tool_call_count: usize,
    pub file_change_count: usize,
    pub total_tokens: usize,
    pub line_count: usize,
    pub corrupt_lines: usize,
    pub has_user_event: bool,
    pub size_bytes: u64,
    pub indexed_at: Option<DateTime<Utc>>,
    /// tool/skill 이름별 호출 수 (function_call/custom_tool_call의 name 집계)
    pub tool_usage: HashMap<String, usize>,
    /// 단어 빈도 — 카테고리(conversation/reasoning/tools) → 단어 → 수.
    /// insights --words 용. 스캐너가 summary::tokenize_words로 채운다.
    pub word_counts: HashMap<String, HashMap<String, usize>>,
}

/// JSONL 레코드 타입 (Codex)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum JsonlRecord {
    SessionMeta { payload: SessionMetaPayload },
    ResponseItem { payload: ResponseItemPayload },
    EventMsg { payload: EventMsgPayload },
    #[serde(other)]
    Other,
}

/// Session 메타데이터 페이로드
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetaPayload {
    pub cwd: Option<String>,
    pub model_provider: Option<String>,
    pub cli_version: Option<String>,
    pub source: Option<String>,
    pub git: Option<GitInfo>,
    pub id: Option<String>,
}

/// Git 정보
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitInfo {
    pub commit_hash: Option<String>,
    pub branch: Option<String>,
    pub repository_url: Option<String>,
}

/// ResponseItem 페이로드
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseItemPayload {
    #[serde(rename = "type")]
    pub item_type: Option<String>,
    pub content: Option<serde_json::Value>,
    pub usage: Option<serde_json::Value>,
}

/// EventMsg 페이로드
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMsgPayload {
    #[serde(rename = "type")]
    pub event_type: Option<String>,
    pub model_context_window: Option<String>,
}

/// 압축된 세션 정보
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedSession {
    pub original: PathBuf,
    pub compressed: PathBuf,
    pub checksum_sha256: String,
    pub date: Option<String>,
    pub size_bytes: u64,
    pub compressed_size_bytes: u64,
}

/// DB에서 읽어온 archived 세션 (restore 대상)
#[derive(Debug, Clone, Serialize)]
pub struct ArchivedSessionRow {
    pub session_id: String,
    /// 원본 jsonl 경로 = 복원 대상
    pub path: PathBuf,
    pub date: Option<String>,
    /// .zst 압축본 절대경로
    pub compressed_path: PathBuf,
    pub checksum_sha256: String,
}

/// 세션 정보 (archive 대상/표시용 최소 정보)
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub path: PathBuf,
    pub date: Option<NaiveDate>,
    pub size_bytes: u64,
    pub session_id: Option<String>,
    pub model_provider: Option<String>,
    pub cli_version: Option<String>,
    /// .zst 보관본 존재 여부
    pub archived: bool,
}

/// 메뉴/기능이 속한 백엔드 (cli/tui 공유)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Backend {
    Codex,
    Hermes,
    Both,
}

impl Backend {
    /// 주어진 설정에서 이 백엔드가 활성인지
    pub fn is_enabled(self, config: &crate::config::Config) -> bool {
        match self {
            Backend::Codex => config.enabled_codex,
            Backend::Hermes => config.enabled_hermes,
            Backend::Both => config.enabled_codex || config.enabled_hermes,
        }
    }
}

/// Hermes 세션 레코드
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HermesSession {
    pub session_id: String,
    pub source_file: String,
    pub path: PathBuf,
    pub model: String,
    pub base_url: String,
    pub platform: String,
    pub session_start: Option<DateTime<Utc>>,
    pub last_updated: Option<DateTime<Utc>>,
    pub message_count: usize,
    pub title: String,
    pub first_user_prompt: String,
    pub summary: String,
    pub keywords: Vec<String>,
    pub keyword_text: String,
    pub tool_usage: HashMap<String, usize>,
    pub project_context: Vec<String>,
    pub large_content: Vec<LargeContentItem>,
}

/// 대용량 컨텐츠 아이템
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LargeContentItem {
    pub role: String,
    pub tool_name: String,
    pub size: usize,
    pub preview: String,
}

/// 요약 레이어
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryLayer {
    pub schema_version: u32,
    pub generated_at: DateTime<Utc>,
    pub sessions_dir: PathBuf,
    pub session_count: usize,
    pub sessions: Vec<HermesSession>,
}

/// FTS5 인덱스
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fts5Index {
    pub schema_version: u32,
    pub generated_at: DateTime<Utc>,
    pub sessions_dir: PathBuf,
    pub session_count: usize,
    pub index: Vec<Fts5Entry>,
}

/// FTS5 엔트리
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fts5Entry {
    pub session_id: String,
    pub source_file: String,
    pub path: PathBuf,
    pub session_start: Option<DateTime<Utc>>,
    pub last_updated: Option<DateTime<Utc>>,
    pub message_count: usize,
    pub title: String,
    pub first_user_prompt: String,
    pub summary: String,
    pub keywords: Vec<String>,
    pub keyword_text: String,
    pub tool_usage: HashMap<String, usize>,
    pub project_context: Vec<String>,
    pub search_text: String,
}

/// 민감정보 탐지 결과
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensitiveFile {
    pub path: PathBuf,
    pub date: Option<String>,
    pub size_bytes: u64,
    pub patterns: Vec<String>,
}

/// 통계 정보
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    pub total_sessions: usize,
    pub total_size_bytes: u64,
    pub by_provider: HashMap<String, usize>,
    pub by_month: HashMap<String, usize>,
    pub by_model: HashMap<String, usize>,
}
