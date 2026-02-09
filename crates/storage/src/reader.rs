//! Session Reader
//!
//! Reads full conversation content from Claude Code JSONL session files.

use crate::models::{ClaudeSession, SessionMessage};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use tracing::warn;

/// Reader for Claude Code session JSONL files.
pub struct SessionReader;

impl SessionReader {
    /// Build a ClaudeSession from a JSONL file by reading its first few lines
    /// and file metadata. This is the single source of truth for session info.
    pub fn read_session_metadata(jsonl_path: &Path) -> Option<ClaudeSession> {
        let file = File::open(jsonl_path).ok()?;
        let mtime = file.metadata().ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0)
                .unwrap_or_default().to_rfc3339())
            .unwrap_or_default();

        let reader = BufReader::new(file);
        let mut session_id = None;
        let mut cwd = None;
        let mut git_branch = None;
        let mut created_at = None;
        let mut first_prompt = None;

        for line in reader.lines().take(20) {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if line.trim().is_empty() {
                continue;
            }
            let val: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            // Extract metadata from any line that has it
            if session_id.is_none() {
                session_id = val.get("sessionId").and_then(|v| v.as_str()).map(String::from);
            }
            if cwd.is_none() {
                cwd = val.get("cwd").and_then(|v| v.as_str()).map(String::from);
            }
            if git_branch.is_none() {
                git_branch = val.get("gitBranch").and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty()).map(String::from);
            }
            if created_at.is_none() {
                created_at = val.get("timestamp").and_then(|v| v.as_str()).map(String::from);
            }

            // Extract first user prompt
            if first_prompt.is_none() {
                let msg_type = val.get("type").and_then(|v| v.as_str());
                if msg_type == Some("user") {
                    if let Ok(msg) = serde_json::from_value::<SessionMessage>(val) {
                        if let Some(text) = Self::extract_text(msg.content()) {
                            if !Self::is_system_noise(&text) {
                                first_prompt = Some(text);
                            }
                        }
                    }
                    // If we have everything, stop reading
                    if session_id.is_some() && cwd.is_some() && first_prompt.is_some() {
                        break;
                    }
                    continue;
                }
            }

            // Stop early if we have all metadata
            if session_id.is_some() && cwd.is_some() && first_prompt.is_some() {
                break;
            }
        }

        // session_id is required — derive from filename if not in content
        let session_id = session_id.or_else(|| {
            jsonl_path.file_stem()
                .and_then(|s| s.to_str())
                .map(String::from)
        })?;

        Some(ClaudeSession {
            session_id,
            project_path: cwd.unwrap_or_default(),
            jsonl_path: jsonl_path.to_string_lossy().to_string(),
            first_prompt,
            summary: None,
            message_count: 0,
            created_at: created_at.unwrap_or_default(),
            modified_at: mtime,
            git_branch,
        })
    }

    /// Read all messages from a JSONL session file.
    pub fn read_session(jsonl_path: &Path) -> Result<Vec<SessionMessage>, std::io::Error> {
        let file = File::open(jsonl_path)?;
        let reader = BufReader::new(file);
        let mut messages = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            match serde_json::from_str::<SessionMessage>(&line) {
                Ok(msg) => messages.push(msg),
                Err(e) => {
                    warn!("Failed to parse JSONL line: {}", e);
                }
            }
        }

        Ok(messages)
    }

    /// Extract the first user prompt text from a JSONL file.
    pub fn extract_first_prompt(jsonl_path: &Path) -> Option<String> {
        let file = File::open(jsonl_path).ok()?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            let line = line.ok()?;
            if line.trim().is_empty() {
                continue;
            }
            let msg: SessionMessage = match serde_json::from_str(&line) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if msg.message_type != "user" {
                continue;
            }
            if let Some(text) = Self::extract_text(msg.content()) {
                if Self::is_system_noise(&text) {
                    continue;
                }
                return Some(text);
            }
        }
        None
    }

    /// Stream messages from a JSONL file (lazy iterator).
    pub fn stream_session(jsonl_path: &Path) -> Result<impl Iterator<Item = SessionMessage>, std::io::Error> {
        let file = File::open(jsonl_path)?;
        let reader = BufReader::new(file);

        Ok(reader.lines().filter_map(|line| {
            let line = line.ok()?;
            if line.trim().is_empty() {
                return None;
            }
            serde_json::from_str::<SessionMessage>(&line).ok()
        }))
    }

    /// Extract human-readable conversation from messages.
    pub fn format_conversation(messages: &[SessionMessage]) -> String {
        let mut output = String::new();

        for msg in messages {
            match msg.message_type.as_str() {
                "user" | "assistant" => {
                    let role = msg.role().unwrap_or(&msg.message_type);
                    if let Some(text) = Self::extract_text(msg.content()) {
                        output.push_str(&format!("**{}**: {}\n\n", role, text));
                    }
                }
                "summary" => {
                    if let Some(summary) = &msg.summary {
                        output.push_str(&format!("---\n**Summary**: {}\n---\n\n", summary));
                    }
                }
                _ => {}
            }
        }

        output
    }

    /// Check if a message is system noise that should not be used as a title.
    ///
    /// Filters out Claude Code injected XML tags (system-reminder, local-command-*,
    /// command-name, etc.) and interrupt markers.
    fn is_system_noise(text: &str) -> bool {
        let trimmed = text.trim_start();
        trimmed.starts_with("[Request interrupted")
            || Self::starts_with_xml_tag(trimmed)
    }

    /// Check if text starts with an XML-like tag (e.g. `<foo>`, `<foo-bar>`).
    fn starts_with_xml_tag(text: &str) -> bool {
        let bytes = text.as_bytes();
        if bytes.first() != Some(&b'<') || bytes.get(1) == Some(&b' ') {
            return false;
        }
        // Look for closing '>' within the first 60 chars
        text[1..text.len().min(60)]
            .find('>')
            .map_or(false, |i| {
                // Ensure contents between < and > look like a tag name (letters, dashes, underscores)
                text[1..=i].trim_end_matches('/').chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            })
    }

    /// Extract plain text from message content.
    fn extract_text(content: Option<&serde_json::Value>) -> Option<String> {
        match content {
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            Some(serde_json::Value::Array(arr)) => {
                let mut texts = Vec::new();
                for item in arr {
                    match item.get("type").and_then(|v| v.as_str()) {
                        Some("text") => {
                            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                texts.push(text.to_string());
                            }
                        }
                        Some("tool_use") => {
                            if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                                texts.push(format!("[Tool: {}]", name));
                            }
                        }
                        Some("tool_result") => {
                            texts.push("[Tool Result]".to_string());
                        }
                        _ => {}
                    }
                }
                if texts.is_empty() {
                    None
                } else {
                    Some(texts.join("\n"))
                }
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_text_string() {
        let content = serde_json::Value::String("Hello world".to_string());
        assert_eq!(SessionReader::extract_text(Some(&content)), Some("Hello world".to_string()));
    }

    #[test]
    fn test_extract_text_array() {
        let content = serde_json::json!([
            {"type": "text", "text": "Hello"},
            {"type": "text", "text": "World"}
        ]);
        assert_eq!(SessionReader::extract_text(Some(&content)), Some("Hello\nWorld".to_string()));
    }

    #[test]
    fn test_extract_text_none() {
        assert_eq!(SessionReader::extract_text(None), None);
    }
}
