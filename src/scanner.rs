//! Phase 1: Codex 세션 스캔 및 인덱싱

use crate::config::Config;
use crate::db::SessionDb;
use crate::error::{Error, Result};
use crate::types::CodexSessionMeta;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use walkdir::WalkDir;

/// Codex 세션 스캐너
pub struct CodexScanner {
    config: Config,
    progress: bool,
}

impl CodexScanner {
    /// 새 스캐너 생성
    pub fn new(config: Config) -> Self {
        Self {
            config,
            progress: true,
        }
    }

    /// 진행률 표시 설정
    pub fn with_progress(mut self, progress: bool) -> Self {
        self.progress = progress;
        self
    }

    /// 모든 세션 스캔
    pub fn scan_all(&self) -> Result<Vec<CodexSessionMeta>> {
        let sessions_dir = &self.config.codex_sessions;

        if !sessions_dir.exists() {
            return Err(Error::PathNotFound(sessions_dir.clone()));
        }

        let mut results = Vec::new();
        let mut total_files = 0;
        let mut processed = 0;

        // 먼저 파일 수 계산
        for entry in WalkDir::new(sessions_dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.path().extension().and_then(|s| s.to_str()) == Some("jsonl") {
                let filename = entry.file_name().to_string_lossy();
                if filename.starts_with("rollout-") {
                    total_files += 1;
                }
            }
        }

        if self.progress {
            println!("Found {} JSONL files", total_files);
        }

        // 스캔 시작
        for entry in WalkDir::new(sessions_dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                continue;
            }

            let filename = path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            if !filename.starts_with("rollout-") {
                continue;
            }

            processed += 1;

            match self.extract_session_meta(path) {
                Ok(meta) => {
                    results.push(meta);
                    if self.progress && processed % 500 == 0 {
                        println!("  scanned {}/{}...", processed, total_files);
                    }
                }
                Err(e) => {
                    eprintln!("  ERROR processing {}: {}", path.display(), e);
                }
            }
        }

        if self.progress {
            println!("Extracted metadata for {} sessions", results.len());
        }

        Ok(results)
    }

    /// 단일 세션 메타데이터 추출 (스트리밍)
    fn extract_session_meta(&self, path: &Path) -> Result<CodexSessionMeta> {
        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::InvalidPath(path.to_path_buf()))?;

        let (date, session_id) = self.parse_filename(filename)?;

        let file = File::open(path)
            .map_err(|e| Error::Io(e))?;
        let reader = BufReader::new(file);

        let mut meta = CodexSessionMeta {
            path: path.to_path_buf(),
            filename: filename.to_string(),
            session_id,
            date,
            cwd: None,
            first_user_prompt: None,
            model_provider: None,
            cli_version: None,
            source: None,
            model: None,
            git_sha: None,
            git_branch: None,
            git_origin_url: None,
            tool_call_count: 0,
            file_change_count: 0,
            total_tokens: 0,
            line_count: 0,
            corrupt_lines: 0,
            has_user_event: false,
            indexed_at: None,
        };

        let mut found_first_prompt = false;

        for line in reader.lines() {
            let line = line.map_err(|e| Error::Io(e))?;
            let line = line.trim();

            if line.is_empty() {
                continue;
            }

            meta.line_count += 1;

            // 첫 번째 사용자 프롬프트 추출 (최초 1회만)
            if !found_first_prompt {
                if let Ok(record) = serde_json::from_str::<serde_json::Value>(line) {
                    if let Some(record_type) = record.get("type").and_then(|t| t.as_str()) {
                        if record_type == "response_item" {
                            if let Some(payload) = record.get("payload") {
                                if let Some(content) = payload.get("content").and_then(|c| c.as_array()) {
                                    for item in content {
                                        if let Some(item_type) = item.get("type").and_then(|t| t.as_str()) {
                                            if item_type == "message" {
                                                let role = item.get("role").and_then(|r| r.as_str());
                                                if role == Some("user") {
                                                    if let Some(item_content) = item.get("content").and_then(|c| c.as_array()) {
                                                        let texts: Vec<String> = item_content.iter()
                                                            .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
                                                            .map(|s| s.to_string())
                                                            .collect();

                                                        if !texts.is_empty() {
                                                            let prompt = texts.join("\n");
                                                            meta.first_user_prompt = Some(prompt.chars().take(2000).collect());
                                                            found_first_prompt = true;
                                                        }
                                                    }
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // JSON 파싱
            let record: serde_json::Value = serde_json::from_str(line)
                .unwrap_or_else(|_| serde_json::json!({ "_corrupt": true }));

            if record.get("_corrupt").is_some() {
                meta.corrupt_lines += 1;
                continue;
            }

            let record_type = record.get("type").and_then(|t| t.as_str());

            match record_type {
                Some("session_meta") => {
                    if let Some(payload) = record.get("payload") {
                        meta.cwd = payload.get("cwd").and_then(|v| v.as_str()).map(String::from);
                        meta.model_provider = payload.get("model_provider").and_then(|v| v.as_str()).map(String::from);
                        meta.cli_version = payload.get("cli_version").and_then(|v| v.as_str()).map(String::from);
                        meta.source = payload.get("source").and_then(|v| v.as_str()).map(String::from);

                        if let Some(git) = payload.get("git") {
                            meta.git_sha = git.get("commit_hash").and_then(|v| v.as_str()).map(String::from);
                            meta.git_branch = git.get("branch").and_then(|v| v.as_str()).map(String::from);
                            meta.git_origin_url = git.get("repository_url").and_then(|v| v.as_str()).map(String::from);
                        }
                    }
                }
                Some("response_item") => {
                    if let Some(payload) = record.get("payload") {
                        let item_type = payload.get("type").and_then(|v| v.as_str());
                        if item_type == Some("function_call") || item_type == Some("custom_tool_call") {
                            meta.tool_call_count += 1;
                        }

                        // 토큰 수 집계
                        if let Some(usage) = payload.get("usage") {
                            if let Some(obj) = usage.as_object() {
                                let input_tokens = obj.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                                let output_tokens = obj.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                                meta.total_tokens += (input_tokens + output_tokens) as usize;
                            } else if let Some(n) = usage.as_u64() {
                                meta.total_tokens += n as usize;
                            }
                        }
                    }
                }
                Some("event_msg") => {
                    if let Some(payload) = record.get("payload") {
                        let event_type = payload.get("type").and_then(|v| v.as_str());

                        if event_type == Some("task_started") {
                            meta.has_user_event = true;
                            if let Some(mcw) = payload.get("model_context_window") {
                                meta.model = mcw.as_str().map(String::from);
                            }
                        } else if event_type == Some("patch_apply_end") {
                            meta.file_change_count += 1;
                        }
                    }
                }
                _ => {}
            }
        }

        Ok(meta)
    }

    /// 파일명에서 날짜와 세션 ID 추출
    fn parse_filename(&self, filename: &str) -> Result<(Option<String>, String)> {
        // rollout-2026-03-03T14-37-44-uuid.jsonl
        let stem = filename.strip_suffix(".jsonl")
            .ok_or_else(|| Error::InvalidArgument(format!("Invalid JSONL filename: {}", filename)))?;

        let session_id = stem.strip_prefix("rollout-")
            .unwrap_or(stem)
            .to_string();

        let parts: Vec<&str> = session_id.split('T').collect();
        let date = if parts.len() >= 2 {
            let date_part = parts[0];
            if date_part.len() == 10 && date_part.chars().filter(|&c| c == '-').count() == 2 {
                Some(date_part.to_string())
            } else {
                None
            }
        } else {
            None
        };

        Ok((date, session_id))
    }

    /// 스캔 결과를 DB에 인덱싱
    pub fn index_sessions(&self, metas: Vec<CodexSessionMeta>) -> Result<()> {
        let db = SessionDb::new(&self.config.codex_index_db)?;

        db.begin_transaction()?;

        for meta in &metas {
            if let Err(e) = db.upsert_session(meta) {
                eprintln!("Error indexing session {}: {}", meta.session_id, e);
            }
        }

        db.commit()?;
        drop(db);

        if self.progress {
            println!("Indexed {} sessions to {}", metas.len(), self.config.codex_index_db.display());
        }

        Ok(())
    }

    /// 분석 리포트 생성
    pub fn run_analysis(&self) -> Result<()> {
        let db = SessionDb::new(&self.config.codex_index_db)?;

        println!("\n{}", "=".repeat(60));
        println!("SESSION ANALYSIS");
        println!("{}", "=".repeat(60));

        // 전체 세션 수
        let total = db.count_sessions()?;
        println!("\nTotal sessions indexed: {}", total);

        // 월별 통계
        println!("\n--- Volume by Month ---");
        println!("{:<12} {:>8} {:>10} {:>12} {:>12}", "Month", "Sessions", "Tool Calls", "File Changes", "Tokens (M)");

        let by_month = db.count_by_month()?;
        for row in by_month {
            println!("{:<12} {:>8} {:>10} {:>12} {:>12.1}",
                row.0, row.1, row.2, row.3, row.4 as f64 / 1e6);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs::create_dir_all;

    #[test]
    fn test_parse_filename() {
        let config = Config::default();
        let scanner = CodexScanner::new(config);

        let (date, session_id) = scanner.parse_filename("rollout-2026-03-03T14-37-44-uuid.jsonl").unwrap();
        assert_eq!(date, Some("2026-03-03".to_string()));
        assert_eq!(session_id, "2026-03-03T14-37-44-uuid");
    }

    #[test]
    fn test_extract_session_meta() {
        let dir = TempDir::new().unwrap();
        let test_file = dir.path().join("rollout-2026-03-03T14-37-44-test.jsonl");

        let test_content = r#"{"type":"session_meta","payload":{"cwd":"/test","model_provider":"test-provider","cli_version":"1.0.0","git":{"commit_hash":"abc123","branch":"main"}}}
{"type":"response_item","payload":{"type":"function_call"}}
{"type":"event_msg","payload":{"type":"task_started","model_context_window":"gpt-4"}}
{"type":"response_item","payload":{"usage":{"input_tokens":100,"output_tokens":50}}}
"#;

        std::fs::write(&test_file, test_content).unwrap();

        let config = Config::default();
        let scanner = CodexScanner::new(config);
        let meta = scanner.extract_session_meta(&test_file).unwrap();

        assert_eq!(meta.cwd, Some("/test".to_string()));
        assert_eq!(meta.model_provider, Some("test-provider".to_string()));
        assert_eq!(meta.tool_call_count, 1);
        assert_eq!(meta.has_user_event, true);
        assert_eq!(meta.total_tokens, 150);
    }
}
