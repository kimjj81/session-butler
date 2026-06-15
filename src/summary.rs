//! Phase 4: Hermes 세션 요약 및 분석

use crate::config::Config;
use crate::error::{Error, Result};
use crate::types::{Fts5Entry, Fts5Index, HermesSession, LargeContentItem, SummaryLayer};
use crate::util;
use chrono::{DateTime, Utc};
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

/// 불용어 목록 (insights 단어 빈도 분석에서도 재사용)
pub(crate) const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "can", "do", "for", "from",
    "has", "have", "i", "if", "in", "is", "it", "json", "me", "my", "of", "on",
    "or", "path", "please", "python", "python3", "read", "session", "sessions",
    "the", "this", "that", "to", "tool", "tools", "true", "false", "user", "assistant",
    "with", "write", "you", "your",
];

/// Hermes 세션 요약 생성기
pub struct SummaryBuilder {
    config: Config,
    token_re: Regex,
    file_re: Regex,
    path_re: Regex,
    tool_tag_re: Regex,
}

impl SummaryBuilder {
    /// 새 요약 생성기
    pub fn new(config: Config) -> Result<Self> {
        let token_re = Regex::new(r"[A-Za-z0-9_가-힣./-]{2,}")
            .map_err(|e| Error::Other(format!("Invalid token regex: {}", e)))?;
        let file_re = Regex::new(r"\b[\w./-]+\.(?:py|js|ts|tsx|md|json|ya?ml|toml|sh|bash|sql|csv|txt|xml|zst|sqlite)\b")
            .map_err(|e| Error::Other(format!("Invalid file regex: {}", e)))?;
        let path_re = Regex::new(r##"(?:~/?|/)[^\s"'`<>]+"##)
            .map_err(|e| Error::Other(format!("Invalid path regex: {}", e)))?;
        let tool_tag_re = Regex::new(r"^\[(?P<tool>[A-Za-z0-9_.:-]+)\]")
            .map_err(|e| Error::Other(format!("Invalid tool tag regex: {}", e)))?;

        Ok(Self {
            config,
            token_re,
            file_re,
            path_re,
            tool_tag_re,
        })
    }

    /// 세션 레이어 빌드
    pub fn build_summary_layer(&self) -> Result<(SummaryLayer, Fts5Index)> {
        let sessions_dir = &self.config.hermes_sessions;

        if !sessions_dir.exists() {
            return Err(Error::PathNotFound(sessions_dir.clone()));
        }

        let mut hermes_sessions = Vec::new();
        let mut fts_entries = Vec::new();

        // session_*.json 파일 찾기
        for entry in fs::read_dir(sessions_dir)
            .map_err(|e| Error::Io(e))?
        {
            let entry = entry.map_err(|e| Error::Io(e))?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }

            let filename = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            if !filename.starts_with("session_") {
                continue;
            }

            // 세션 로드
            let session_data: Value = fs::read_to_string(&path)
                .map_err(|e| Error::Io(e))
                .and_then(|s| serde_json::from_str(&s).map_err(|e| Error::Json(e)))?;

            let hermes_session = self.build_session_record(&path, &session_data)?;

            let fts_entry = Fts5Entry {
                session_id: hermes_session.session_id.clone(),
                source_file: hermes_session.source_file.clone(),
                path: hermes_session.path.clone(),
                session_start: hermes_session.session_start,
                last_updated: hermes_session.last_updated,
                message_count: hermes_session.message_count,
                title: hermes_session.title.clone(),
                first_user_prompt: hermes_session.first_user_prompt.clone(),
                summary: hermes_session.summary.clone(),
                keywords: hermes_session.keywords.clone(),
                keyword_text: hermes_session.keyword_text.clone(),
                tool_usage: hermes_session.tool_usage.clone(),
                project_context: hermes_session.project_context.clone(),
                search_text: self.build_search_text(&hermes_session),
            };

            hermes_sessions.push(hermes_session);
            fts_entries.push(fts_entry);
        }

        let generated_at = Utc::now();

        let summary_layer = SummaryLayer {
            schema_version: 1,
            generated_at,
            sessions_dir: sessions_dir.clone(),
            session_count: hermes_sessions.len(),
            sessions: hermes_sessions,
        };

        let fts_index = Fts5Index {
            schema_version: 1,
            generated_at,
            sessions_dir: sessions_dir.clone(),
            session_count: fts_entries.len(),
            index: fts_entries,
        };

        Ok((summary_layer, fts_index))
    }

    /// 단일 세션 레코드 빌드
    fn build_session_record(&self, path: &Path, session_data: &Value) -> Result<HermesSession> {
        static EMPTY_MESSAGES: serde_json::Value = serde_json::json!([]);

        let messages = session_data.get("messages")
            .and_then(|m| m.as_array())
            .unwrap_or_else(|| EMPTY_MESSAGES.as_array().unwrap());

        let first_user_prompt = self.extract_first_user_prompt(messages);

        let keywords = self.extract_keywords(messages);
        let keyword_text = keywords.join(" ");

        let tool_usage = self.extract_tool_usage(messages);

        let project_context = self.extract_project_context(messages);

        let summary = self.extract_summary(messages);

        let large_content = self.extract_large_content(messages);

        let session_id_value = path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(HermesSession {
            session_id: session_data.get("session_id")
                .and_then(|s| s.as_str())
                .unwrap_or(&session_id_value)
                .to_string(),
            source_file: util::nfc(
                path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(""),
            ),
            path: PathBuf::from(util::nfc(&path.to_string_lossy())),
            model: session_data.get("model")
                .and_then(|m| m.as_str())
                .unwrap_or("")
                .to_string(),
            base_url: session_data.get("base_url")
                .and_then(|u| u.as_str())
                .unwrap_or("")
                .to_string(),
            platform: session_data.get("platform")
                .and_then(|p| p.as_str())
                .unwrap_or("")
                .to_string(),
            session_start: session_data.get("session_start")
                .and_then(|s| s.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            last_updated: session_data.get("last_updated")
                .and_then(|s| s.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|dt| dt.with_timezone(&Utc)),
            message_count: messages.len(),
            title: self.truncate_text(&first_user_prompt, 120),
            first_user_prompt,
            summary,
            keywords,
            keyword_text,
            tool_usage,
            project_context,
            large_content,
        })
    }

    /// 첫 번째 사용자 프롬프트 추출
    fn extract_first_user_prompt(&self, messages: &[Value]) -> String {
        for msg in messages {
            if msg.get("role").and_then(|r| r.as_str()) == Some("user") {
                if let Some(content) = msg.get("content") {
                    return self.flatten_content(content).trim().to_string();
                }
            }
        }
        String::new()
    }

    /// 컨텐츠 평탄화
    fn flatten_content(&self, content: &Value) -> String {
        if content.is_null() {
            return String::new();
        }

        if let Some(s) = content.as_str() {
            return s.to_string();
        }

        if let Some(arr) = content.as_array() {
            let parts: Vec<String> = arr.iter()
                .filter_map(|item| {
                    if let Some(obj) = item.as_object() {
                        if let Some(text) = obj.get("text").and_then(|t| t.as_str()) {
                            return Some(text.to_string());
                        }
                    }
                    item.as_str().map(|s| s.to_string())
                })
                .collect();
            return parts.join(" ");
        }

        if let Some(obj) = content.as_object() {
            if let Some(text) = obj.get("text").and_then(|t| t.as_str()) {
                return text.to_string();
            }
            return serde_json::to_string(obj).unwrap_or_default();
        }

        content.to_string()
    }

    /// 텍스트 자르기
    fn truncate_text(&self, text: &str, limit: usize) -> String {
        let text = self.clean_text(text);
        text.chars().take(limit).collect()
    }

    /// 텍스트 정리 — Python clean_text와 동일 (연속 공백을 단일 스페이스로 정리).
    /// title/truncate에 token 기반 분할을 쓰면 1글자 한글·구두점이 사라지므로
    /// Python 기준에 맞춰 공백 정리만 수행한다.
    fn clean_text(&self, text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// 요약 추출
    fn extract_summary(&self, messages: &[Value]) -> String {
        let mut events = Vec::new();

        for msg in messages.iter().take(20) {
            let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("unknown");
            let content = self.flatten_content(msg.get("content").unwrap_or(&Value::Null));
            let content = self.truncate_text(&content, 150);

            if content.len() <= 20 {
                continue;
            }

            if role == "user" || role == "assistant" {
                events.push(format!("[{}] {}", role.to_uppercase(), content));
            } else if role == "tool" {
                let tool_name = self.extract_tool_name(&content);
                let prefix = if !tool_name.is_empty() {
                    format!("{}: ", tool_name)
                } else {
                    String::new()
                };
                let tool_content = self.truncate_text(&content, 100);
                events.push(format!("[TOOL] {}{}", prefix, tool_content));
            }

            if events.len() >= 20 {
                break;
            }
        }

        events.iter()
            .map(|e| format!("  {}", e))
            .collect::<Vec<_>>()
            .join("\n")
            .chars()
            .take(500)
            .collect()
    }

    /// 툴 이름 추출
    fn extract_tool_name(&self, content: &str) -> String {
        let content = content.trim();
        if let Some(caps) = self.tool_tag_re.captures(content) {
            return caps.name("tool").map(|m| m.as_str().to_string()).unwrap_or_default();
        }

        String::new()
    }

    /// 툴 사용량 추출
    fn extract_tool_usage(&self, messages: &[Value]) -> HashMap<String, usize> {
        let mut counter: HashMap<String, usize> = HashMap::new();

        for msg in messages {
            // role == "tool" 메시지
            if msg.get("role").and_then(|r| r.as_str()) == Some("tool") {
                let content = self.flatten_content(msg.get("content").unwrap_or(&Value::Null));
                let tool_name = self.extract_tool_name(&content);

                if !tool_name.is_empty() {
                    *counter.entry(tool_name).or_default() += 1;
                    continue;
                }

                if let Some(name) = msg.get("name").or(msg.get("tool_name")).and_then(|n| n.as_str()) {
                    *counter.entry(name.to_string()).or_default() += 1;
                }
            }

            // tool_calls 배열
            if let Some(tool_calls) = msg.get("tool_calls").and_then(|t| t.as_array()) {
                for call in tool_calls {
                    if let Some(function) = call.get("function").and_then(|f| f.as_object()) {
                        if let Some(name) = function.get("name").and_then(|n| n.as_str()) {
                            *counter.entry(name.to_string()).or_default() += 1;
                        }
                    }
                }
            }
        }

        // 정렬
        let mut sorted: Vec<_> = counter.into_iter().collect();
        sorted.sort_by(|a, b| {
            match b.1.cmp(&a.1) {
                std::cmp::Ordering::Equal => a.0.cmp(&b.0),
                other => other,
            }
        });

        sorted.into_iter().collect()
    }

    /// 대용량 컨텐츠 추출
    fn extract_large_content(&self, messages: &[Value]) -> Vec<LargeContentItem> {
        let mut large_items = Vec::new();

        for msg in messages {
            let content = self.flatten_content(msg.get("content").unwrap_or(&Value::Null));

            if content.len() > 5000 {
                large_items.push(LargeContentItem {
                    role: msg.get("role").and_then(|r| r.as_str()).unwrap_or("unknown").to_string(),
                    tool_name: self.extract_tool_name(&content),
                    size: content.len(),
                    preview: format!("{}...", self.truncate_text(&content, 200)),
                });
            }
        }

        large_items
    }

    /// 경로 추출
    fn extract_paths(&self, text: &str) -> Vec<String> {
        let mut results = Vec::new();

        // path_re와 file_re에서 모두 추출
        for pattern in [&self.path_re, &self.file_re] {
            for mat in pattern.find_iter(text) {
                let mut candidate = mat.as_str();

                // 수동으로 문자 제거
                let trim_chars = ['.', ',', ';', ':', ')', ']', '}', '>'];
                candidate = candidate.trim_start_matches(trim_chars);
                candidate = candidate.trim_end_matches(trim_chars);

                if candidate.is_empty() {
                    continue;
                }

                if candidate.contains("://") || candidate.starts_with("data:") {
                    continue;
                }

                results.push(candidate.to_string());
            }
        }

        results
    }

    /// 프로젝트 컨텍스트 추출
    fn extract_project_context(&self, messages: &[Value]) -> Vec<String> {
        let mut seen = HashSet::new();
        let mut items = Vec::new();

        for msg in messages {
            let content = self.flatten_content(msg.get("content").unwrap_or(&Value::Null));

            for candidate in self.extract_paths(&content) {
                if !seen.contains(&candidate) {
                    seen.insert(candidate.clone());
                    items.push(candidate);

                    if items.len() >= 25 {
                        return items;
                    }
                }
            }
        }

        items
    }

    /// 키워드 추출
    fn extract_keywords(&self, messages: &[Value]) -> Vec<String> {
        let mut counter: HashMap<String, usize> = HashMap::new();

        for msg in messages {
            let role = msg.get("role").and_then(|r| r.as_str());
            let content = self.flatten_content(msg.get("content").unwrap_or(&Value::Null));

            if content.is_empty() {
                continue;
            }

            // 툴 메시지에서 툴명 추출
            if role == Some("tool") {
                let tool_name = self.extract_tool_name(&content);
                if !tool_name.is_empty() {
                    *counter.entry(tool_name).or_default() += 5;
                }
            }

            // 경로/파일명 추출
            for candidate in self.extract_paths(&content) {
                if let Some(file_name) = PathBuf::from(&candidate).file_name() {
                    if let Some(name) = file_name.to_str() {
                        *counter.entry(name.to_string()).or_default() += 4;
                    }
                }

                if let Some(stem) = PathBuf::from(&candidate).file_stem() {
                    if let Some(name) = stem.to_str() {
                        *counter.entry(name.to_string()).or_default() += 2;
                    }
                }
            }

            // 토큰 추출
            for mat in self.token_re.find_iter(&content) {
                let mut token_str = mat.as_str();

                // 문자 제거
                let trim_chars = ['.', '_', '-', '/'];
                token_str = token_str.trim_start_matches(trim_chars);
                token_str = token_str.trim_end_matches(trim_chars);

                if token_str.is_empty() {
                    continue;
                }

                let normalized = if token_str.is_ascii() {
                    token_str.to_lowercase()
                } else {
                    token_str.to_string()
                };

                if STOP_WORDS.contains(&normalized.as_str()) || normalized.chars().all(|c| c.is_ascii_digit()) {
                    continue;
                }

                *counter.entry(normalized).or_default() += 1;
            }
        }

        // 정렬
        let mut sorted: Vec<_> = counter.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));

        sorted.into_iter()
            .take(40)
            .map(|(k, _)| k)
            .collect()
    }

    /// 검색 텍스트 빌드
    fn build_search_text(&self, record: &HermesSession) -> String {
        let tool_usage_keys: Vec<&str> = record.tool_usage.keys()
            .map(|k| k.as_str())
            .collect();
        let tool_usage_str = tool_usage_keys.join(" ");

        let keywords_str = record.keywords.join(" ");
        let project_context_str = record.project_context.join(" ");

        let parts = vec![
            &record.first_user_prompt,
            &record.summary,
            &keywords_str,
            &project_context_str,
            &tool_usage_str,
            &record.model,
            &record.platform,
        ];

        parts.into_iter()
            .filter(|p| !p.is_empty())
            .map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(5000)
            .collect()
    }

    /// 요약 레이어 파일에 저장
    pub fn save_summary_layer(&self, summary_layer: &SummaryLayer) -> Result<()> {
        let path = &self.config.summary_layer;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| Error::Io(e))?;
        }

        let content = serde_json::to_string_pretty(summary_layer)
            .map_err(|e| Error::Json(e))?;

        fs::write(path, content)
            .map_err(|e| Error::Io(e))?;

        println!("Wrote {} ({} sessions)", path.display(), summary_layer.session_count);
        Ok(())
    }

    /// FTS5 인덱스 파일에 저장
    pub fn save_fts5_index(&self, fts_index: &Fts5Index) -> Result<()> {
        let path = &self.config.fts5_index;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| Error::Io(e))?;
        }

        let content = serde_json::to_string_pretty(fts_index)
            .map_err(|e| Error::Json(e))?;

        fs::write(path, content)
            .map_err(|e| Error::Io(e))?;

        println!("Wrote {} ({} sessions)", path.display(), fts_index.session_count);
        Ok(())
    }

    /// 전체 파이프라인 실행 (요약 + FTS 인덱스)
    pub fn run_pipeline(&self) -> Result<()> {
        let (summary_layer, fts_index) = self.build_summary_layer()?;
        self.save_summary_layer(&summary_layer)?;
        self.save_fts5_index(&fts_index)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn test_extract_first_user_prompt() {
        let dir = TempDir::new().unwrap();
        let mut config = Config::default();
        config.hermes_sessions = dir.path().to_path_buf();

        let builder = SummaryBuilder::new(config).unwrap();

        let messages = vec![
            json!({"role": "system", "content": "You are a helpful assistant."}),
            json!({"role": "user", "content": "Hello, world!"}),
        ];

        let prompt = builder.extract_first_user_prompt(&messages);
        assert_eq!(prompt, "Hello, world!");
    }

    #[test]
    fn test_extract_keywords() {
        let dir = TempDir::new().unwrap();
        let mut config = Config::default();
        config.hermes_sessions = dir.path().to_path_buf();

        let builder = SummaryBuilder::new(config).unwrap();

        let messages = vec![
            json!({"role": "user", "content": "Help me with Python and Rust code"}),
        ];

        let keywords = builder.extract_keywords(&messages);
        assert!(keywords.iter().any(|k| k.contains("python") || k.contains("rust")));
    }

    #[test]
    fn test_extract_tool_usage() {
        let dir = TempDir::new().unwrap();
        let mut config = Config::default();
        config.hermes_sessions = dir.path().to_path_buf();

        let builder = SummaryBuilder::new(config).unwrap();

        let messages = vec![
            json!({"role": "tool", "content": "[read_file] Reading file...", "name": "read_file"}),
        ];

        let tool_usage = builder.extract_tool_usage(&messages);
        assert!(tool_usage.contains_key("read_file"));
    }

    #[test]
    fn test_truncate_text() {
        let dir = TempDir::new().unwrap();
        let mut config = Config::default();
        config.hermes_sessions = dir.path().to_path_buf();

        let builder = SummaryBuilder::new(config).unwrap();

        let long_text = "a".repeat(200);
        let truncated = builder.truncate_text(&long_text, 100);
        assert_eq!(truncated.len(), 100);
    }
}
