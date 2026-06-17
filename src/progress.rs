//! 진행률 표시 (indicatif 래퍼 + Progress trait).
//!
//! 긴 작업(scan/archive/compact)에서 진행률 바/스피너를 표시한다.
//! stderr가 터미널이 아니면(파이프/TUI의 gag 캡처 등) 자동으로 숨겨
//! 제어 문자가 결과에 섞이지 않도록 한다.
//!
//! `Progress` trait으로 추상화해 CLI(TerminalProgress=indicatif)와
//! GUI(EventProgress=Tauri 이벤트) 모두 같은 호출 코드를 공유한다.

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use std::io::IsTerminal;
use std::time::Duration;

/// stderr가 터미널일 때만 그리고, 아니면 숨김.
fn draw_target() -> ProgressDrawTarget {
    if std::io::stderr().is_terminal() {
        ProgressDrawTarget::stderr()
    } else {
        ProgressDrawTarget::hidden()
    }
}

/// 진행률 출력 추상화. CLI(indicatif)와 GUI(이벤트) 양쪽 구현체를 둔다.
/// `Arc<dyn Progress>`로 스레드 경계(spawn_blocking)를 넘겨 공유 가능.
pub trait Progress: Send + Sync {
    /// 카운트 기반 바. `len` = 전체 항목 수.
    fn bar(&self, len: u64, msg: &str) -> Box<dyn Bar>;
    /// 부가 진행(예: DB 인덱싱)용 스피너.
    fn spinner(&self, msg: &str) -> Box<dyn Bar>;
}

/// 진행 항목(handle). indicatif ProgressBar와 동일한 inc/finish 인터페이스.
pub trait Bar: Send + Sync {
    fn inc(&self, n: u64);
    fn finish(&self);
}

/// 터미널 진행률(indicatif). 터미널이 아니면 자동 숨김 — CLI/TUI용.
pub struct TerminalProgress;

impl Progress for TerminalProgress {
    fn bar(&self, len: u64, msg: &str) -> Box<dyn Bar> {
        let pb = ProgressBar::new(len);
        pb.set_draw_target(draw_target());
        pb.set_style(
            ProgressStyle::with_template(
                "{spinner:.green} {msg} [{bar:30.cyan/blue}] {human_pos}/{human_len} ({percent}%) ETA {eta}",
            )
            .unwrap()
            .progress_chars("=>-"),
        );
        pb.set_message(msg.to_string());
        Box::new(TerminalBar(pb))
    }

    fn spinner(&self, msg: &str) -> Box<dyn Bar> {
        let pb = ProgressBar::new_spinner();
        pb.set_draw_target(draw_target());
        pb.set_style(ProgressStyle::with_template("{spinner:.green} {msg}").unwrap());
        pb.set_message(msg.to_string());
        pb.enable_steady_tick(Duration::from_millis(120));
        Box::new(TerminalBar(pb))
    }
}

struct TerminalBar(ProgressBar);
impl Bar for TerminalBar {
    fn inc(&self, n: u64) {
        self.0.inc(n);
    }
    fn finish(&self) {
        self.0.finish();
    }
}

/// 아무것도 표시하지 않는 진행률 — 비활성/캡처 환경용.
pub struct NoopProgress;

impl Progress for NoopProgress {
    fn bar(&self, _len: u64, _msg: &str) -> Box<dyn Bar> {
        Box::new(NoopBar)
    }
    fn spinner(&self, _msg: &str) -> Box<dyn Bar> {
        Box::new(NoopBar)
    }
}

struct NoopBar;
impl Bar for NoopBar {
    fn inc(&self, _n: u64) {}
    fn finish(&self) {}
}

// ---- 레거시 free 함수 (archive/compact 가 Phase 2에서 trait로 전환 전까지 사용) ----

/// 카운트 기반 진행률 바. `len` = 전체 항목 수.
pub fn bar(len: u64, msg: &str) -> ProgressBar {
    let pb = ProgressBar::new(len);
    pb.set_draw_target(draw_target());
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} {msg} [{bar:30.cyan/blue}] {human_pos}/{human_len} ({percent}%) ETA {eta}",
        )
        .unwrap()
        .progress_chars("=>-"),
    );
    pb.set_message(msg.to_string());
    pb
}

/// 부가 진행(예: DB 인덱싱)용 스피너.
pub fn spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_draw_target(draw_target());
    pb.set_style(ProgressStyle::with_template("{spinner:.green} {msg}").unwrap());
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(Duration::from_millis(120));
    pb
}

/// 터미널 여부와 무관하게 아무것도 그리지 않는 no-op 바.
pub fn hidden() -> ProgressBar {
    ProgressBar::hidden()
}

/// `show`가 true이고 stderr가 터미널일 때만 보이는 바.
pub fn bar_if(len: u64, msg: &str, show: bool) -> ProgressBar {
    if show {
        bar(len, msg)
    } else {
        hidden()
    }
}
