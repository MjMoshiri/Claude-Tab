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

    pub fn is_alive(&self, session_id: &str) -> bool {
        self.pty.is_alive(session_id)
    }

    /// Writes a sanitized `text` followed by a single `\r` into the session's PTY.
    ///
    /// Workflow content is treated as untrusted: ESC, BEL, DEL and most C0
    /// control bytes are stripped so a malicious or buggy workflow can't
    /// inject CSI/OSC sequences (e.g. clipboard manipulation, cursor moves)
    /// or smuggle "Enter" early. Tab/CR/LF are kept and any trailing CR/LF
    /// is collapsed to one terminating `\r`.
    pub fn write_user_prompt(&self, session_id: &str, text: &str) -> Result<(), InjectError> {
        let sanitized: String = text
            .chars()
            .filter(|c| !is_dangerous_control(*c))
            .collect();
        let trimmed = sanitized.trim_end_matches(|c: char| c == '\r' || c == '\n');
        let mut payload = String::with_capacity(trimmed.len() + 1);
        payload.push_str(trimmed);
        payload.push('\r');
        self.pty
            .write_data(session_id, payload.as_bytes())
            .map_err(|e| match e {
                PtyError::NotFound(id) => InjectError::NotFound(id),
                other => InjectError::Write(other.to_string()),
            })
    }
}

fn is_dangerous_control(c: char) -> bool {
    match c {
        '\t' | '\n' | '\r' => false,
        '\x00'..='\x1F' | '\x7F' => true,
        _ => false,
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

    #[test]
    fn strips_esc_and_bell() {
        // ESC[31m, BEL, DEL all forbidden.
        let raw = "hello\x1b[31mworld\x07!\x7f";
        let sanitized: String = raw.chars().filter(|c| !is_dangerous_control(*c)).collect();
        assert_eq!(sanitized, "hello[31mworld!");
    }

    #[test]
    fn keeps_tab_newline_carriage_return() {
        let raw = "a\tb\nc\rd";
        let sanitized: String = raw.chars().filter(|c| !is_dangerous_control(*c)).collect();
        assert_eq!(sanitized, raw);
    }

    #[test]
    fn collapses_trailing_lf_to_single_cr() {
        // Simulate the production pipeline manually since we can't write
        // to a real PTY here.
        let text = "do thing\n";
        let sanitized: String = text.chars().filter(|c| !is_dangerous_control(*c)).collect();
        let trimmed = sanitized.trim_end_matches(|c: char| c == '\r' || c == '\n');
        let mut payload = trimmed.to_string();
        payload.push('\r');
        assert_eq!(payload, "do thing\r");
    }
}
