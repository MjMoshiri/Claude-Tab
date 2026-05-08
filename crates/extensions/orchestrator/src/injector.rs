//! PTY injector — auto-types stage prompts into an existing Claude Code session.

use claude_tabs_pty::{PtyError, PtyManager};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InjectError {
    #[error("session not found: {0}")]
    NotFound(String),
    #[error("write failed: {0}")]
    Write(String),
}

pub struct Injector {
    pty: Arc<PtyManager>,
}

impl Injector {
    pub fn new(pty: Arc<PtyManager>) -> Self {
        Self { pty }
    }

    /// Writes `text` followed by a carriage return into the session's PTY.
    /// `\r` is appended unless the input already ends in one.
    pub fn write_user_prompt(&self, session_id: &str, text: &str) -> Result<(), InjectError> {
        let mut payload = text.to_string();
        if !payload.ends_with('\r') {
            payload.push('\r');
        }
        self.pty
            .write_data(session_id, payload.as_bytes())
            .map_err(|e| match e {
                PtyError::NotFound(id) => InjectError::NotFound(id),
                other => InjectError::Write(other.to_string()),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_session_returns_not_found() {
        let mgr = Arc::new(PtyManager::new());
        let injector = Injector::new(mgr);
        let err = injector
            .write_user_prompt("nonexistent-session", "hi")
            .unwrap_err();
        assert!(matches!(err, InjectError::NotFound(_)));
    }
}
