//! Error types for reflection database parsing.

use thiserror::Error;

/// Errors that can occur while loading or parsing reflection metadata.
#[derive(Debug, Error)]
pub enum ReflectionError {
    #[error("failed to parse reflection dump JSON: {0}")]
    Json(#[from] serde_json::Error),

    #[error("failed to read reflection dump file: {0}")]
    Io(#[from] std::io::Error),
}
