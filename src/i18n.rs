//! i18n (한국어/영어) — 직접 구현. 글로벌 언어 상태 + 메시지 함수.

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
        Lang::Ko => format!("{}개 세션을 압축하고 원본 .jsonl을 삭제합니다 (--move)", n),
        Lang::En => format!("Compressing {} session(s) and deleting originals (--move)", n),
    }
}

pub fn archive_start_keep(n: usize) -> String {
    match lang() {
        Lang::Ko => format!("{}개 세션을 압축합니다 (원본 보존)", n),
        Lang::En => format!("Compressing {} session(s) (originals kept)", n),
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
        Lang::Ko => format!("[dry-run] 압축 대상 {}개 세션 (원본 {})", n, mode),
        Lang::En => format!("[dry-run] {} session(s) to archive (originals {})", n, mode),
    }
}

pub fn archive_summary(n: usize, a: f64, b: f64, p: f64) -> String {
    match lang() {
        Lang::Ko => format!("{}개 세션 압축 ({:.1}GB → {:.1}GB, {:.0}% 축소)", n, a, b, p),
        Lang::En => format!("Archived {} sessions ({:.1}GB -> {:.1}GB, {:.0}% reduction)", n, a, b, p),
    }
}

pub fn archive_skipped(n: usize) -> String {
    match lang() {
        Lang::Ko => format!("건너뜀: {}개 세션", n),
        Lang::En => format!("Skipped: {} sessions", n),
    }
}

pub fn restore_start_purge(n: usize) -> String {
    match lang() {
        Lang::Ko => format!("{}개 복원 + 보관본 삭제 (--purge)", n),
        Lang::En => format!("Restoring {} + deleting archives (--purge)", n),
    }
}

pub fn restore_start_keep(n: usize) -> String {
    match lang() {
        Lang::Ko => format!("{}개 복원 (.zst 보존, 재복원 가능)", n),
        Lang::En => format!("Restoring {} (.zst kept, re-restorable)", n),
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
        Lang::Ko => format!("[dry-run] 복원 대상 {}개 세션 (.zst {})", n, mode),
        Lang::En => format!("[dry-run] {} session(s) to restore (.zst {})", n, mode),
    }
}

pub fn restore_summary(n: usize) -> String {
    match lang() {
        Lang::Ko => format!("{}개 세션 복원", n),
        Lang::En => format!("Restored {} sessions", n),
    }
}

pub fn scan_found(n: usize) -> String {
    match lang() {
        Lang::Ko => format!("{}개 JSONL 파일 발견", n),
        Lang::En => format!("Found {} JSONL files", n),
    }
}

pub fn scan_scanning(done: usize, total: usize) -> String {
    match lang() {
        Lang::Ko => format!("  스캔 중 {}/{}...", done, total),
        Lang::En => format!("  scanned {}/{}...", done, total),
    }
}

pub fn scan_scanned(n: usize) -> String {
    match lang() {
        Lang::Ko => format!("{}개 세션 메타데이터 추출", n),
        Lang::En => format!("Extracted metadata for {} sessions", n),
    }
}

pub fn scan_indexed(n: usize, path: &str) -> String {
    match lang() {
        Lang::Ko => format!("{}개 세션 인덱싱 → {}", n, path),
        Lang::En => format!("Indexed {} sessions to {}", n, path),
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
        (Lang::Ko, "summarize") => "Hermes 세션 요약 및 FTS5 인덱스",
        (Lang::Ko, "pipeline") => "전체 파이프라인 실행",
        (Lang::En, "scan") => "Scan Codex sessions & index to SQLite",
        (Lang::En, "archive") => "zstd compress (--move deletes originals)",
        (Lang::En, "restore") => "Restore from archive (--purge deletes .zst)",
        (Lang::En, "list") => "List sessions",
        (Lang::En, "stats") => "Session statistics",
        (Lang::En, "compact") => "Compaction + sensitive-info scan",
        (Lang::En, "summarize") => "Summarize Hermes sessions + FTS5",
        (Lang::En, "pipeline") => "Run the full pipeline",
        _ => "",
    }
}
