//! 진행률 표시 (indicatif 래퍼).
//!
//! 긴 작업(scan/archive/compact)에서 진행률 바/스피너를 표시한다.
//! stderr가 터미널이 아니면(파이프/TUI의 gag 캡처 등) 자동으로 숨겨
//! 제어 문자가 결과에 섞이지 않도록 한다.

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
