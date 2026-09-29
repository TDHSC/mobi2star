//! Shared lexicon/provenance model, checked byte ranges and audit primitives.
//! No I/O side effects except the explicitly named bounded read/hash functions.
#![forbid(unsafe_code)]

pub mod bytes;
pub mod labels;
pub mod model;
pub mod safety;
pub use labels::*;
pub use model::*;
pub use safety::*;

use thiserror::Error;
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("malformed input: {0}")]
    Malformed(String),
    #[error("unsupported feature: {0}")]
    Unsupported(String),
    #[error("completeness check failed: {0}")]
    Incomplete(String),
    #[error("resource limit exceeded: {0}")]
    Limit(String),
    #[error("verification failed: {0}")]
    Verify(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

impl Error {
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Malformed(_) => 3,
            Self::Unsupported(_) => 4,
            Self::Incomplete(_) | Self::Limit(_) => 5,
            Self::Io(_) => 6,
            Self::Verify(_) | Self::Json(_) => 7,
        }
    }
    pub fn code(&self) -> &'static str {
        match self {
            Self::Malformed(_) => "MALFORMED",
            Self::Unsupported(_) => "UNSUPPORTED",
            Self::Incomplete(_) => "INCOMPLETE",
            Self::Limit(_) => "LIMIT",
            Self::Io(_) => "IO",
            Self::Verify(_) => "VERIFY",
            Self::Json(_) => "JSON",
        }
    }
}
