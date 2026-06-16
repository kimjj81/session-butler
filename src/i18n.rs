//! i18n (한국어/영어) — 직접 구현. 글로벌 언어 상태 + 메시지 함수.

use crate::util;
use std::sync::Mutex;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Ko,
    En,
}

static LANG: Mutex<Lang> = Mutex::new(Lang::Ko);

/// 언어 코드("ko"/"en")로 설정
pub fn set_lang(code: &str) {
    let l = if code.eq_ignore_ascii_case("en") { Lang::En } else { Lang::Ko };
    *LANG.lock().unwrap() = l;
}

pub fn lang() -> Lang {
    *LANG.lock().unwrap()
}

// ---- 고정 메시지 ----

pub fn skip_scan_empty() -> &'static str {
    match lang() {
        Lang::Ko => "경고: DB 인덱스가 비어 있습니다. scan을 먼저 실행하거나 --skip-scan을 제거하세요.",
        Lang::En => "warning: DB index is empty. Run scan first or remove --skip-scan.",
    }
}

pub fn mode_delete() -> &'static str {
    match lang() {
        Lang::Ko => "삭제",
        Lang::En => "delete",
    }
}

pub fn mode_keep() -> &'static str {
    match lang() {
        Lang::Ko => "보존",
        Lang::En => "keep",
    }
}

pub fn first_run_prompt(detected_ko: bool) -> &'static str {
    match detected_ko {
        true => "언어를 한국어로 감지했습니다. 한국어를 사용합니다? [Y/n] ",
        false => "Detected language: English. Use English? [Y/n] ",
    }
}

// ---- 포맷 메시지 ----

pub fn backend_disabled(name: &str) -> String {
    match lang() {
        Lang::Ko => format!(
            "{} 백엔드 비활성 — 건너뜁니다 (--no-{} 또는 CODEX/HERMES_ENABLED 확인)",
            name, name
        ),
        Lang::En => format!(
            "{} backend disabled — skipping (check --no-{} or CODEX/HERMES_ENABLED)",
            name, name
        ),
    }
}

pub fn archive_start_move(n: usize) -> String {
    match lang() {
        Lang::Ko => format!("{}개 세션을 압축하고 원본 .jsonl을 삭제합니다 (--move)", util::fmt_int(n as i64)),
        Lang::En => format!("Compressing {} session(s) and deleting originals (--move)", util::fmt_int(n as i64)),
    }
}

pub fn archive_start_keep(n: usize) -> String {
    match lang() {
        Lang::Ko => format!("{}개 세션을 압축합니다 (원본 보존)", util::fmt_int(n as i64)),
        Lang::En => format!("Compressing {} session(s) (originals kept)", util::fmt_int(n as i64)),
    }
}

pub fn archive_start_none() -> &'static str {
    match lang() {
        Lang::Ko => "압축 대상이 없습니다.",
        Lang::En => "No sessions to archive.",
    }
}

pub fn archive_start_dryrun(n: usize, mode: &str) -> String {
    match lang() {
        Lang::Ko => format!("[dry-run] 압축 대상 {}개 세션 (원본 {})", util::fmt_int(n as i64), mode),
        Lang::En => format!("[dry-run] {} session(s) to archive (originals {})", util::fmt_int(n as i64), mode),
    }
}

pub fn archive_summary(n: usize, a: f64, b: f64, p: f64) -> String {
    match lang() {
        Lang::Ko => format!("{}개 세션 압축 ({:.1}GB → {:.1}GB, {:.0}% 축소)", util::fmt_int(n as i64), a, b, p),
        Lang::En => format!("Archived {} sessions ({:.1}GB -> {:.1}GB, {:.0}% reduction)", util::fmt_int(n as i64), a, b, p),
    }
}

pub fn archive_skipped(n: usize) -> String {
    match lang() {
        Lang::Ko => format!("건너뜀: {}개 세션", util::fmt_int(n as i64)),
        Lang::En => format!("Skipped: {} sessions", util::fmt_int(n as i64)),
    }
}

pub fn restore_start_purge(n: usize) -> String {
    match lang() {
        Lang::Ko => format!("{}개 복원 + 보관본 삭제 (--purge)", util::fmt_int(n as i64)),
        Lang::En => format!("Restoring {} + deleting archives (--purge)", util::fmt_int(n as i64)),
    }
}

pub fn restore_start_keep(n: usize) -> String {
    match lang() {
        Lang::Ko => format!("{}개 복원 (.zst 보존, 재복원 가능)", util::fmt_int(n as i64)),
        Lang::En => format!("Restoring {} (.zst kept, re-restorable)", util::fmt_int(n as i64)),
    }
}

pub fn restore_start_none() -> &'static str {
    match lang() {
        Lang::Ko => "복원 대상이 없습니다.",
        Lang::En => "No sessions to restore.",
    }
}

pub fn restore_start_dryrun(n: usize, mode: &str) -> String {
    match lang() {
        Lang::Ko => format!("[dry-run] 복원 대상 {}개 세션 (.zst {})", util::fmt_int(n as i64), mode),
        Lang::En => format!("[dry-run] {} session(s) to restore (.zst {})", util::fmt_int(n as i64), mode),
    }
}

pub fn restore_summary(n: usize) -> String {
    match lang() {
        Lang::Ko => format!("{}개 세션 복원", util::fmt_int(n as i64)),
        Lang::En => format!("Restored {} sessions", util::fmt_int(n as i64)),
    }
}

pub fn archive_progress_label() -> String {
    match lang() {
        Lang::Ko => "압축 중".to_string(),
        Lang::En => "Archiving".to_string(),
    }
}

pub fn restore_progress_label() -> String {
    match lang() {
        Lang::Ko => "복원 중".to_string(),
        Lang::En => "Restoring".to_string(),
    }
}

pub fn compact_progress_label() -> String {
    match lang() {
        Lang::Ko => "compaction 중".to_string(),
        Lang::En => "Compacting".to_string(),
    }
}

pub fn scan_sensitive_progress_label() -> String {
    match lang() {
        Lang::Ko => "민감정보 스캔 중".to_string(),
        Lang::En => "Scanning sensitive info".to_string(),
    }
}

pub fn scan_found(n: usize) -> String {
    match lang() {
        Lang::Ko => format!("{}개 JSONL 파일 발견", util::fmt_int(n as i64)),
        Lang::En => format!("Found {} JSONL files", util::fmt_int(n as i64)),
    }
}

pub fn scan_scanning(done: usize, total: usize) -> String {
    match lang() {
        Lang::Ko => format!("  스캔 중 {}/{}...", util::fmt_int(done as i64), util::fmt_int(total as i64)),
        Lang::En => format!("  scanned {}/{}/...", util::fmt_int(done as i64), util::fmt_int(total as i64)),
    }
}

pub fn scan_scanned(n: usize) -> String {
    match lang() {
        Lang::Ko => format!("{}개 세션 메타데이터 추출", util::fmt_int(n as i64)),
        Lang::En => format!("Extracted metadata for {} sessions", util::fmt_int(n as i64)),
    }
}

pub fn scan_indexed(n: usize, path: &str) -> String {
    match lang() {
        Lang::Ko => format!("{}개 세션 인덱싱 → {}", util::fmt_int(n as i64), path),
        Lang::En => format!("Indexed {} sessions to {}", util::fmt_int(n as i64), path),
    }
}

pub fn scan_progress_label() -> String {
    match lang() {
        Lang::Ko => "스캔 중".to_string(),
        Lang::En => "Scanning".to_string(),
    }
}

pub fn scan_indexing_label() -> String {
    match lang() {
        Lang::Ko => "인덱싱 중".to_string(),
        Lang::En => "Indexing".to_string(),
    }
}

pub fn tui_desc(id: &str) -> &'static str {
    match (lang(), id) {
        (Lang::Ko, "scan") => "Codex 세션 스캔 및 SQLite 인덱싱",
        (Lang::Ko, "archive") => "zstd 압축 (--move 원본 삭제)",
        (Lang::Ko, "restore") => "압축 파일 복원 (--purge 보관본 삭제)",
        (Lang::Ko, "list") => "세션 목록 표시",
        (Lang::Ko, "stats") => "세션 통계 표시",
        (Lang::Ko, "compact") => "세션 compaction 및 민감정보 탐지",
        (Lang::Ko, "summarize") => "세션 요약 및 FTS5 인덱스",
        (Lang::Ko, "insights") => "사용 인사이트 (tool/프로젝트/추세/단어)",
        (Lang::Ko, "pipeline") => "전체 파이프라인 실행",
        (Lang::En, "scan") => "Scan Codex sessions & index to SQLite",
        (Lang::En, "archive") => "zstd compress (--move deletes originals)",
        (Lang::En, "restore") => "Restore from archive (--purge deletes .zst)",
        (Lang::En, "list") => "List sessions",
        (Lang::En, "stats") => "Session statistics",
        (Lang::En, "compact") => "Compaction + sensitive-info scan",
        (Lang::En, "summarize") => "Summarize sessions + FTS5",
        (Lang::En, "insights") => "Usage insights (tools/projects/trends/words)",
        (Lang::En, "pipeline") => "Run the full pipeline",
        _ => "",
    }
}

// ---- 인사이트 리포트 문자열 ----

pub fn insights_title() -> &'static str {
    match lang() {
        Lang::Ko => "사용 인사이트 (Usage Insights)",
        Lang::En => "Usage Insights",
    }
}

pub fn insights_empty() -> &'static str {
    match lang() {
        Lang::Ko => "데이터 없음 — 먼저 scan을 실행해 색인하세요.",
        Lang::En => "No data — run scan to index sessions first.",
    }
}

pub fn insights_window(days: u64) -> String {
    match lang() {
        Lang::Ko => {
            if days == 0 {
                "기간: 전체 기간 (0=전체)".to_string()
            } else {
                format!("기간: 최근 {}일", days)
            }
        }
        Lang::En => {
            if days == 0 {
                "Window: all time (0=all)".to_string()
            } else {
                format!("Window: last {} days", days)
            }
        }
    }
}

/// 인사이트 섹션 헤더/라벨. 미지정 id → 빈 문자열.
pub fn insights_section(id: &str) -> &'static str {
    match (lang(), id) {
        (Lang::Ko, "overview") => "개요",
        (Lang::Ko, "top_tools") => "자주 쓴 tool/skill",
        (Lang::Ko, "least_tools") => "덜 쓴 tool/skill",
        (Lang::Ko, "projects") => "프로젝트별",
        (Lang::Ko, "trend_daily") => "일별 추세 (세션/토큰/스킬/최빈단어)",
        (Lang::Ko, "trend_weekly") => "주별 추세 (세션/토큰/스킬/최빈단어)",
        (Lang::Ko, "trend_monthly") => "월별 추세 (세션/토큰/스킬/최빈단어)",
        (Lang::Ko, "activity") => "요일별 활동",
        (Lang::Ko, "words") => "자주 쓴 단어",
        (Lang::Ko, "leaders") => "토큰 상위 세션",
        (Lang::Ko, "sessions") => "세션",
        (Lang::Ko, "tokens") => "토큰",
        (Lang::Ko, "tool_calls") => "툴 호출",
        (Lang::Ko, "file_changes") => "파일 변경",
        (Lang::Ko, "projects_lbl") => "프로젝트",
        (Lang::Ko, "tools_distinct") => "tool 종류",
        (Lang::Ko, "archived") => "보관(archived)",
        (Lang::Ko, "date_range") => "기간",
        (Lang::Ko, "peak_hour") => "피크 시간",
        (Lang::Ko, "repo") => "repo",
        (Lang::Ko, "month") => "월",
        (Lang::Ko, "bucket") => "구간",
        (Lang::Ko, "top_skill") => "대표 스킬",
        (Lang::Ko, "top_words") => "최빈 단어",

        (Lang::En, "overview") => "Overview",
        (Lang::En, "top_tools") => "Most-used tools/skills",
        (Lang::En, "least_tools") => "Least-used tools/skills",
        (Lang::En, "projects") => "By project",
        (Lang::En, "trend_daily") => "Daily trend (sessions/tokens/skill/top words)",
        (Lang::En, "trend_weekly") => "Weekly trend (sessions/tokens/skill/top words)",
        (Lang::En, "trend_monthly") => "Monthly trend (sessions/tokens/skill/top words)",
        (Lang::En, "activity") => "Activity by weekday",
        (Lang::En, "words") => "Top words",
        (Lang::En, "leaders") => "Top sessions by tokens",
        (Lang::En, "sessions") => "Sessions",
        (Lang::En, "tokens") => "Tokens",
        (Lang::En, "tool_calls") => "Tool calls",
        (Lang::En, "file_changes") => "File changes",
        (Lang::En, "projects_lbl") => "Projects",
        (Lang::En, "tools_distinct") => "Distinct tools",
        (Lang::En, "archived") => "Archived",
        (Lang::En, "date_range") => "Date range",
        (Lang::En, "peak_hour") => "Peak hour",
        (Lang::En, "repo") => "repo",
        (Lang::En, "month") => "Month",
        (Lang::En, "bucket") => "bucket",
        (Lang::En, "top_skill") => "Top skill",
        (Lang::En, "top_words") => "Top words",
        _ => "",
    }
}

/// 단어 섹션 헤더. 카테고리(conversation/reasoning/tools/first-prompt)에 따라 표시.
pub fn insights_words_header(id: &str) -> &'static str {
    match (lang(), id) {
        (Lang::Ko, "conversation") => "자주 쓴 단어 (대화)",
        (Lang::Ko, "reasoning") => "자주 쓴 단어 (추론)",
        (Lang::Ko, "tools") => "자주 쓴 단어 (도구·출력)",
        (Lang::Ko, "first-prompt") => "자주 쓴 단어 (첫 프롬프트)",
        (Lang::Ko, _) => "자주 쓴 단어",
        (Lang::En, "conversation") => "Top words (conversation)",
        (Lang::En, "reasoning") => "Top words (reasoning)",
        (Lang::En, "tools") => "Top words (tools/output)",
        (Lang::En, "first-prompt") => "Top words (first prompt)",
        (Lang::En, _) => "Top words",
    }
}

/// 기존 인덱스 폴백 안내 — session_words 미백필 시 첫 프롬프트로 표시함을 알림.
pub fn insights_words_fallback_note() -> &'static str {
    match lang() {
        Lang::Ko => "참고: 단어 카테고리 데이터가 없어 첫 프롬프트 기반으로 표시합니다. `scan` 재실행 시 대화/추론/도구별 분석이 활성화됩니다.",
        Lang::En => "Note: word-category data not found; showing first-prompt words. Re-run `scan` to enable conversation/reasoning/tools breakdown.",
    }
}
