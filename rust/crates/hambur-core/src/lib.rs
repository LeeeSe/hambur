use std::time::{SystemTime, UNIX_EPOCH};

pub const DTO_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    InvalidCommand,
    RuntimeClosed,
    InternalError,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvalidCommand => "InvalidCommand",
            Self::RuntimeClosed => "RuntimeClosed",
            Self::InternalError => "InternalError",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HamburError {
    #[error("invalid command: {0}")]
    InvalidCommand(String),
    #[error("runtime is closed")]
    RuntimeClosed,
    #[error("internal error: {0}")]
    Internal(String),
}

impl HamburError {
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidCommand(_) => ErrorCode::InvalidCommand,
            Self::RuntimeClosed => ErrorCode::RuntimeClosed,
            Self::Internal(_) => ErrorCode::InternalError,
        }
    }
}

pub type HamburResult<T> = Result<T, HamburError>;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4())
}
