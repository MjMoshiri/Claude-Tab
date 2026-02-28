//! Storage Crate
//!
//! Provides persistence functionality for Claude Tabs:
//! - `StorageBackend`: Trait for storage implementations
//! - `SqliteBackend`: SQLite-based storage (preferences only)
//! - `SessionScanner`: Scans JSONL session files from ~/.claude/projects/
//! - `SessionReader`: Reads session JSONL files

pub mod migrations;
pub mod models;
pub mod reader;
pub mod scanner;
pub mod sqlite;
pub mod traits;

// Re-export public API
pub use models::{ClaudeSession, DirectoryPreference, MessagePayload, SessionFilter, SessionMessage, SessionMetadata};
pub use reader::SessionReader;
pub use scanner::SessionScanner;
pub use sqlite::SqliteBackend;
pub use traits::StorageBackend;

use thiserror::Error;

/// Storage operation errors.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("initialization error: {0}")]
    Init(String),
    #[error("query error: {0}")]
    Query(String),
    #[error("lock error: {0}")]
    Lock(String),
    #[error("IO error: {0}")]
    Io(String),
}
