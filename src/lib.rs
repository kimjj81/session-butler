//! Session Butler - Codex/Hermes 세션 파일 관리 도구
//!
//! 4단계 파이프라인:
//! - Phase 1: Codex 세션 스캔 및 SQLite 인덱싱
//! - Phase 2: zstd 압축 및 checksum 관리
//! - Phase 3: Compaction 및 민감정보 탐지
//! - Phase 4: Hermes 세션 요약 및 분석

#![allow(clippy::too_many_arguments)]

pub mod archive;
pub mod cli;
pub mod compact;
pub mod config;
pub mod db;
pub mod error;
pub mod scanner;
pub mod summary;
pub mod tui;
pub mod types;
pub mod util;

pub use error::{Error, Result};

/// Session Butler 버전
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
