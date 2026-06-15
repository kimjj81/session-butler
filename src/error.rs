//! Session Butler 에러 타입 정의

use std::path::PathBuf;
use thiserror::Error;

/// Session Butler 결과 타입
pub type Result<T> = std::result::Result<T, Error>;

/// Session Butler 에러 타입
#[derive(Error, Debug)]
pub enum Error {
    /// IO 에러
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON 파싱 에러
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// SQLite 에러
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// 압축 에러
    #[error("Compression error: {0}")]
    Compression(String),

    /// 날짜 파싱 에러
    #[error("Date parsing error: {0}")]
    DateParse(String),

    /// 잘못된 경로
    #[error("Invalid path: {0}")]
    InvalidPath(PathBuf),

    /// 경로를 찾을 수 없음
    #[error("Path not found: {0}")]
    PathNotFound(PathBuf),

    /// 잘못된 인자
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    /// 지원하지 않는 작업
    #[error("Unsupported operation: {0}")]
    UnsupportedOperation(String),

    /// 세션 데이터가 손상됨
    #[error("Corrupted session data in {0}")]
    CorruptedSession(PathBuf),

    /// 민감정보 탐지 실패
    #[error("Sensitive data detection error: {0}")]
    SensitiveDataError(String),

    /// 설정 에러
    #[error("Configuration error: {0}")]
    Config(String),

    /// TUI 에러
    #[error("TUI error: {0}")]
    Tui(String),

    /// 취소됨
    #[error("Operation cancelled")]
    Cancelled,

    /// 기타 에러
    #[error("Unknown error: {0}")]
    Other(String),
}

impl Error {
    /// 압축 에러 생성
    pub fn compression<S: Into<String>>(msg: S) -> Self {
        Error::Compression(msg.into())
    }

    /// 잘못된 인자 에러 생성
    pub fn invalid_arg<S: Into<String>>(msg: S) -> Self {
        Error::InvalidArgument(msg.into())
    }

    /// 지원하지 않는 작업 에러 생성
    pub fn unsupported<S: Into<String>>(msg: S) -> Self {
        Error::UnsupportedOperation(msg.into())
    }
}
