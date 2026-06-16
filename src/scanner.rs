//! Phase 1: Codex 세션 스캔 및 인덱싱

use crate::config::Config;
use crate::db::SessionDb;
use crate::error::{Error, Result};
use crate::i18n;
use crate::types::CodexSessionMeta;
use crate::util;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
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

    /// 모든 세션 스캔 (WalkDir 1패스: 파일 수집 후 추출)
    pub fn scan_all(&self) -> Result<Vec<CodexSessionMeta>> {
        let sessions_dir = &self.config.codex_sessions;

        if !sessions_dir.exists() {
            return Err(Error::PathNotFound(sessions_dir.clone()));
        }

        // 1패스: rollout-*.jsonl 파일 수집
        let files: Vec<PathBuf> = WalkDir::new(sessions_dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|entry| {
                let path = entry.path();
                path.extension().and_then(|s| s.to_str()) == Some("jsonl")
                    && path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .starts_with("rollout-")
            })
            .map(|e| e.path().to_path_buf())
            .collect();

        let total_files = files.len();
        if self.progress {
            println!("{}", i18n::scan_found(total_files));
        }

        // 진행률 바 (터미널일 때만 표시; TUI gag/파이프는 hidden)
        let visible = self.progress && std::io::IsTerminal::is_terminal(&std::io::stderr());
        let pb = crate::progress::bar_if(total_files as u64, &i18n::scan_progress_label(), self.progress);

        // 추출
        let mut results = Vec::new();
        for path in files.iter() {
            pb.inc(1);
            match self.extract_session_meta(path) {
                Ok(meta) => results.push(meta),
                Err(e) => {
                    let msg = format!("  ERROR processing {}: {}", path.display(), e);
                    if visible {
                        pb.println(msg);
                    } else {
                        eprintln!("{}", msg);
                    }
                }
            }
        }
        pb.finish();

        if self.progress {
            println!("{}", i18n::scan_scanned(results.len()));
        }

        Ok(results)
    }

    /// 단일 세션 메타데이터 추출 (스트리밍)
    fn extract_session_meta(&self, path: &Path) -> Result<CodexSessionMeta> {
        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::InvalidPath(path.to_path_buf()))?;

        let (date, session_id) = self.parse_filename(filename)?;

        let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

        let file = File::open(path)
            .map_err(|e| Error::Io(e))?;
        let reader = BufReader::new(file);

        let mut meta = CodexSessionMeta {
            path: PathBuf::from(util::nfc(&path.to_string_lossy())),
            filename: util::nfc(filename),
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
            size_bytes,
            indexed_at: None,
            tool_usage: std::collections::HashMap::new(),
        };

        let mut found_first_prompt = false;

        for line in reader.lines() {
            let line = line.map_err(|e| Error::Io(e))?;
            let line = line.trim();

            if line.is_empty() {
                continue;
            }

            meta.line_count += 1;

            // 첫 번째 사용자 프롬프트 추출 (유효한 첫 user 메시지).
            // 실제 포맷: type==response_item → payload.type=="message" && payload.role=="user",
            // payload.content[] 의 text 필드들. 주입 컨텍스트(AGENTS.md 등)는 건너뛴다.
            if !found_first_prompt {
                if let Ok(record) = serde_json::from_str::<serde_json::Value>(line) {
                    let payload = record.get("payload");
                    let is_user_msg = record.get("type").and_then(|t| t.as_str()) == Some("response_item")
                        && payload.and_then(|p| p.get("type")).and_then(|t| t.as_str()) == Some("message")
                        && payload.and_then(|p| p.get("role")).and_then(|t| t.as_str()) == Some("user");

                    if is_user_msg {
                        let texts: Vec<String> = payload
                            .and_then(|p| p.get("content"))
                            .and_then(|c| c.as_array())
                            .map(|arr| arr.iter()
                                .filter_map(|item| item.get("text").and_then(|t| t.as_str()).map(|s| s.to_string()))
                                .collect())
                            .unwrap_or_default();

                        if !texts.is_empty() {
                            let prompt = texts.join("\n");
                            if !looks_like_injected_context(&prompt) {
                                meta.first_user_prompt = Some(prompt.chars().take(2000).collect());
                                found_first_prompt = true;
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
                        meta.cwd = payload.get("cwd").and_then(|v| v.as_str()).map(|s| util::nfc(s));
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
                            // tool/skill 이름 집계 (있다면)
                            if let Some(name) = payload.get("name").and_then(|v| v.as_str()) {
                                *meta.tool_usage.entry(name.to_string()).or_insert(0) += 1;
                            }
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
                        } else if event_type == Some("token_count") {
                            // payload.info.total_token_usage.total_tokens 는
                            // 세션 누적 토큰(이벤트가 주기적 스냅샷으로 여러 번 발생).
                            // 누적값이므로 합이 아닌 max 로 세션 총토큰을 취한다.
                            if let Some(total) = payload
                                .get("info")
                                .and_then(|i| i.get("total_token_usage"))
                                .and_then(|t| t.get("total_tokens"))
                                .and_then(|v| v.as_u64())
                            {
                                if (total as usize) > meta.total_tokens {
                                    meta.total_tokens = total as usize;
                                }
                            }
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

        let pb = crate::progress::spinner(&i18n::scan_indexing_label());
        db.begin_transaction()?;

        for meta in &metas {
            if let Err(e) = db.upsert_session(meta) {
                eprintln!("Error indexing session {}: {}", meta.session_id, e);
            }
            pb.inc(1);
        }

        db.commit()?;
        drop(db);
        pb.finish();

        if self.progress {
            println!("{}", i18n::scan_indexed(metas.len(), &self.config.codex_index_db.display().to_string()));
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

/// 주입 컨텍스트(AGENTS.md, 시스템 지시문 등) 휴리스틱 감지.
/// first_user_prompt 노이즈 제거용 — 진짜 사용자 프롬프트를 찾을 때까지 건너뛴다.
fn looks_like_injected_context(text: &str) -> bool {
    let trimmed = text.trim_start();
    let lower = text.to_ascii_lowercase();
    trimmed.starts_with("# agents.md")
        || lower.contains("agents.md instructions")
        || lower.contains("<instructions>")
        || lower.contains("<user_instructions>")
        || lower.contains("<system")
        || lower.contains("<environment_context>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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

        // 실제 포맷: payload.type=="message"+role=="user"+content[].type=="input_text"
        // r##"..."## 사용: 본문에 "# 가 포함되어 r#" 가 조기 종료되는 것을 방지
        let test_content = r##"{"type":"session_meta","payload":{"cwd":"/test","model_provider":"test-provider","cli_version":"1.0.0","git":{"commit_hash":"abc123","branch":"main"}}}
{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions for /test\n\n<INSTRUCTIONS>\n..."}]}}
{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Fix the login bug in auth.rs"}]}}
{"type":"response_item","payload":{"type":"function_call","name":"exec_command"}}
{"type":"response_item","payload":{"type":"custom_tool_call","name":"apply_patch"}}
{"type":"event_msg","payload":{"type":"task_started","model_context_window":"gpt-4"}}
{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1200,"output_tokens":300,"total_tokens":1500},"last_token_usage":{"input_tokens":600,"output_tokens":150,"total_tokens":750}}}}
"##;

        std::fs::write(&test_file, test_content).unwrap();

        let config = Config::default();
        let scanner = CodexScanner::new(config);
        let meta = scanner.extract_session_meta(&test_file).unwrap();

        assert_eq!(meta.cwd, Some("/test".to_string()));
        assert_eq!(meta.model_provider, Some("test-provider".to_string()));
        assert_eq!(meta.tool_call_count, 2);
        // 주입 컨텍스트를 건너뛰고 진짜 프롬프트 추출
        assert_eq!(meta.first_user_prompt.as_deref(), Some("Fix the login bug in auth.rs"));
        // tool 이름 집계
        assert_eq!(meta.tool_usage.get("exec_command"), Some(&1));
        assert_eq!(meta.tool_usage.get("apply_patch"), Some(&1));
        assert_eq!(meta.has_user_event, true);
        assert_eq!(meta.total_tokens, 1500);
    }
}
