//! Phase 1: Codex 세션 스캔 및 인덱싱

use crate::config::Config;
use crate::db::SessionDb;
use crate::error::{Error, Result};
use crate::i18n;
use crate::progress::{Progress, TerminalProgress};
use crate::types::CodexSessionMeta;
use crate::util;
use regex::Regex;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use walkdir::WalkDir;

/// 단어 빈도 분석용 토큰 정규식 (summary::tokenize_words와 쌍).
const TOKEN_RE: &str = r"[A-Za-z0-9_가-힣./-]{2,}";

/// Codex 세션 스캐너
pub struct CodexScanner {
    config: Config,
    progress: Arc<dyn Progress>,
}

impl CodexScanner {
    /// 새 스캐너 생성 (진행률 = 터미널 indicatif)
    pub fn new(config: Config) -> Self {
        Self {
            config,
            progress: Arc::new(TerminalProgress),
        }
    }

    /// 진행률 구현체 주입 (GUI는 EventProgress). 미지정 시 TerminalProgress.
    pub fn with_progress(mut self, progress: Arc<dyn Progress>) -> Self {
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
        println!("{}", i18n::scan_found(total_files));

        // 진행률 바 — 주입된 Progress 구현체에 따라 터미널(indicatif) 또는 이벤트(GUI).
        let pb = self.progress.bar(total_files as u64, &i18n::scan_progress_label());

        // 단어 토크나이저(대화 본문 집계용) — 파일마다 재컴파일하지 않도록 1회 컴파일.
        let token_re = Regex::new(TOKEN_RE)
            .map_err(|e| Error::Other(format!("token regex: {}", e)))?;

        // 추출
        let mut results = Vec::new();
        for path in files.iter() {
            pb.inc(1);
            match self.extract_session_meta(path, &token_re) {
                Ok(meta) => results.push(meta),
                Err(e) => {
                    let msg = format!("  ERROR processing {}: {}", path.display(), e);
                    self.progress.warn(&msg);
                }
            }
        }
        pb.finish();

        println!("{}", i18n::scan_scanned(results.len()));

        Ok(results)
    }

    /// 단일 세션 메타데이터 추출 (스트리밍). 파일 경로로부터 열어 reader 버전으로 위임.
    pub fn extract_session_meta(&self, path: &Path, token_re: &Regex) -> Result<CodexSessionMeta> {
        let size_bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let file = File::open(path)
            .map_err(|e| Error::Io(e))?;
        self.extract_session_meta_reader(path, BufReader::new(file), size_bytes, token_re)
    }

    /// 단일 세션 메타데이터 추출 코어 — reader 기반.
    /// 활성 세션은 파일 reader, 압축본은 메모리 해제한 bytes reader(`Cursor`)를 넘겨
    /// 동일한 파싱/토큰화 로직(주입 컨텍스트·비밀 필터 포함)을 재사용한다.
    /// `path` 는 메타데이터 기록용 논리 경로(압축본의 경우 DB 의 원본 path). session_id/date 는
    /// filename 에서 파생되지만 transient 단어 분석에서는 호출측이 DB 행의 값을 그대로 쓰므로 미사용.
    pub fn extract_session_meta_reader<R: std::io::BufRead>(
        &self,
        path: &Path,
        reader: R,
        size_bytes: u64,
        token_re: &Regex,
    ) -> Result<CodexSessionMeta> {
        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| Error::InvalidPath(path.to_path_buf()))?;

        let (date, session_id) = self.parse_filename(filename)?;

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
            word_counts: std::collections::HashMap::new(),
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

                        // conversation 단어 집계 (user/assistant 메시지 텍스트).
                        // 주입 컨텍스트(AGENTS.md 등)는 건너뛴다.
                        if item_type == Some("message") {
                            let role = payload.get("role").and_then(|v| v.as_str());
                            if matches!(role, Some("user") | Some("assistant")) {
                                let texts: Vec<String> = payload
                                    .get("content")
                                    .and_then(|c| c.as_array())
                                    .map(|arr| arr.iter()
                                        .filter_map(|item| item.get("text").and_then(|t| t.as_str()).map(|s| s.to_string()))
                                        .collect())
                                    .unwrap_or_default();
                                if !texts.is_empty() {
                                    let joined = texts.join("\n");
                                    if !looks_like_injected_context(&joined) {
                                        add_words(&mut meta, "conversation", &joined, token_re);
                                    }
                                }
                            }
                        }

                        // tools 단어 집계 — 도구 호출 인자/입력만. 비밀이 섞일 수 있는
                        // 출력 본문(function_call_output/custom_tool_call_output)은 색인하지 않는다.
                        // 인자/입력도 looks_secret 게이트를 통과한 안전한 것만.
                        match item_type {
                            Some("function_call") => {
                                if let Some(args) = payload.get("arguments").and_then(|v| v.as_str()) {
                                    let vals = json_string_values(args);
                                    if !looks_secret(&vals) {
                                        add_words(&mut meta, "tools", &vals, token_re);
                                    }
                                }
                            }
                            Some("custom_tool_call") => {
                                if let Some(inp) = payload.get("input").and_then(|v| v.as_str()) {
                                    if !looks_secret(inp) {
                                        add_words(&mut meta, "tools", inp, token_re);
                                    }
                                }
                            }
                            _ => {}
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
                        } else if event_type == Some("agent_reasoning") {
                            // reasoning 단어 집계 (모델 추론 요약 text).
                            if let Some(t) = payload.get("text").and_then(|v| v.as_str()) {
                                add_words(&mut meta, "reasoning", t, token_re);
                            }
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

        let pb = self.progress.spinner(&i18n::scan_indexing_label());
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

        println!("{}", i18n::scan_indexed(metas.len(), &self.config.codex_index_db.display().to_string()));

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

/// 세션 메타의 word_counts[category][word]에 텍스트를 토큰화해 누적.
fn add_words(meta: &mut CodexSessionMeta, category: &str, text: &str, token_re: &Regex) {
    if text.trim().is_empty() {
        return;
    }
    let map = meta.word_counts.entry(category.to_string()).or_default();
    for w in crate::summary::tokenize_words(text, token_re) {
        *map.entry(w).or_insert(0) += 1;
    }
}

/// JSON 문자열(예: function_call.arguments)에서 문자열 값만 결합해 반환.
/// 키(cmd/workdir/call_id 등)가 단어 통계를 오염시키지 않게 한다.
/// 파싱 실패 시 빈 문자열.
fn json_string_values(s: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(s) {
        Ok(serde_json::Value::Object(map)) => map
            .values()
            .filter_map(|v| v.as_str().map(String::from))
            .collect::<Vec<_>>()
            .join(" "),
        Ok(serde_json::Value::String(s)) => s,
        _ => String::new(),
    }
}

/// 자격증명/비밀이 의심되는 텍스트인지 휴리스틱 판정.
/// tools 카테고리 단어 색인 전에 걸러 — 실수로 비밀이 SQLite에 남는 것을 방지.
/// (출력 본문 전체는 이미 색인 제외; 이것은 인자/입력용 추가 안전망.)
fn looks_secret(text: &str) -> bool {
    const SIGNS: &[&str] = &[
        "eyJ",              // JWT
        "sk-",              // OpenAI 계열 키
        "Bearer ",
        "ghp_", "gho_", "github_pat_", // GitHub 토큰
        "AKIA",             // AWS access key id
        "xox",              // Slack
        "_TOKEN=", "_KEY=", "_SECRET=",
    ];
    let lower = text.to_ascii_lowercase();
    lower.contains("api_key")
        || lower.contains("access_token")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("private_key")
        || SIGNS.iter().any(|s| text.contains(s))
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
{"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"grep login auth.rs\",\"workdir\":\"/test\"}"}}
{"type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"auth.rs:42 login validated"}}
{"type":"event_msg","payload":{"type":"agent_reasoning","text":"analyzing the login flow carefully"}}
{"type":"event_msg","payload":{"type":"task_started","model_context_window":"gpt-4"}}
{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1200,"output_tokens":300,"total_tokens":1500},"last_token_usage":{"input_tokens":600,"output_tokens":150,"total_tokens":750}}}}
"##;

        std::fs::write(&test_file, test_content).unwrap();

        let config = Config::default();
        let scanner = CodexScanner::new(config);
        let token_re = Regex::new(TOKEN_RE).unwrap();
        let meta = scanner.extract_session_meta(&test_file, &token_re).unwrap();

        assert_eq!(meta.cwd, Some("/test".to_string()));
        assert_eq!(meta.model_provider, Some("test-provider".to_string()));
        assert_eq!(meta.tool_call_count, 3);
        // 주입 컨텍스트를 건너뛰고 진짜 프롬프트 추출
        assert_eq!(meta.first_user_prompt.as_deref(), Some("Fix the login bug in auth.rs"));
        // tool 이름 집계
        assert_eq!(meta.tool_usage.get("exec_command"), Some(&2));
        assert_eq!(meta.tool_usage.get("apply_patch"), Some(&1));
        assert_eq!(meta.has_user_event, true);
        assert_eq!(meta.total_tokens, 1500);

        // conversation: "Fix the login bug in auth.rs" (the/in 은 불용어)
        let conv = meta.word_counts.get("conversation").expect("conversation category");
        assert_eq!(conv.get("fix"), Some(&1));
        assert_eq!(conv.get("login"), Some(&1));
        assert_eq!(conv.get("bug"), Some(&1));
        assert!(conv.get("agents.md").is_none());

        // reasoning: agent_reasoning "analyzing the login flow carefully"
        let reas = meta.word_counts.get("reasoning").expect("reasoning category");
        assert_eq!(reas.get("analyzing"), Some(&1));
        assert_eq!(reas.get("login"), Some(&1));
        assert_eq!(reas.get("flow"), Some(&1));

        // tools: function_call arguments(문자열 값만). 출력 본문은 privacy상 색인 제외.
        //   args "grep login auth.rs /test" 만 → grep/login/auth.rs/test
        let tools = meta.word_counts.get("tools").expect("tools category");
        assert_eq!(tools.get("grep"), Some(&1));
        assert_eq!(tools.get("login"), Some(&1)); // args 에만 (output 은 제외)
        assert!(tools.get("cmd").is_none()); // JSON 키는 제외됨
        assert!(tools.get("validated").is_none()); // 출력 본문 단어는 색인되지 않음
    }

    #[test]
    fn test_extract_session_meta_reader_matches_path() {
        // reader 진입점(transient 분석이 압축본 메모리 해제에 사용)이 path 버전과
        // 동일한 토큰화 결과를 내는지 검증. 동일 본문을 Cursor로 공급해 비교.
        let test_content = r##"{"type":"session_meta","payload":{"cwd":"/test"}}
{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Fix the login bug in auth.rs"}]}}
{"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"grep login auth.rs\",\"workdir\":\"/test\"}"}}
{"type":"event_msg","payload":{"type":"agent_reasoning","text":"analyzing the login flow carefully"}}
"##;
        let path = std::path::Path::new("/test/rollout-2026-03-03T14-37-44-test.jsonl");
        let config = Config::default();
        let scanner = CodexScanner::new(config);
        let token_re = Regex::new(TOKEN_RE).unwrap();
        let reader = std::io::Cursor::new(test_content.as_bytes());
        let meta = scanner.extract_session_meta_reader(path, reader, test_content.len() as u64, &token_re).unwrap();

        let conv = meta.word_counts.get("conversation").expect("conversation");
        assert_eq!(conv.get("login"), Some(&1));
        assert_eq!(conv.get("auth.rs"), Some(&1));
        let reas = meta.word_counts.get("reasoning").expect("reasoning");
        assert_eq!(reas.get("analyzing"), Some(&1));
        let tools = meta.word_counts.get("tools").expect("tools");
        assert_eq!(tools.get("grep"), Some(&1));
    }
}
