//! Session Scanner
//!
//! Reads Claude Code sessions directly from JSONL files in ~/.claude/projects/.

use crate::models::ClaudeSession;
use crate::reader::SessionReader;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

/// Scanner for Claude Code session files.
pub struct SessionScanner {
    claude_dir: PathBuf,
}

impl SessionScanner {
    /// Create a new scanner with the default Claude directory (~/.claude).
    pub fn new() -> Result<Self, String> {
        let home = std::env::var("HOME")
            .map_err(|_| "HOME environment variable not set".to_string())?;
        Ok(Self {
            claude_dir: PathBuf::from(home).join(".claude"),
        })
    }

    /// Create a scanner with a custom Claude directory (for testing).
    pub fn with_dir(claude_dir: PathBuf) -> Self {
        Self { claude_dir }
    }

    /// Get the projects directory path.
    pub fn projects_dir(&self) -> PathBuf {
        self.claude_dir.join("projects")
    }

    /// Read all sessions by scanning JSONL files across all projects.
    pub fn list_all_sessions(&self) -> Vec<ClaudeSession> {
        let projects_dir = self.projects_dir();
        if !projects_dir.exists() {
            debug!("Projects directory does not exist: {:?}", projects_dir);
            return Vec::new();
        }

        let mut sessions = Vec::new();
        match fs::read_dir(&projects_dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        sessions.extend(Self::scan_project_dir(&path));
                    }
                }
            }
            Err(e) => {
                warn!("Failed to read projects directory: {}", e);
            }
        }

        // Sort by modified_at descending
        sessions.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
        sessions
    }

    /// Scan a project directory for JSONL session files (skip subagents/).
    fn scan_project_dir(project_dir: &Path) -> Vec<ClaudeSession> {
        let entries = match fs::read_dir(project_dir) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };

        let mut sessions = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("jsonl") && path.is_file() {
                if let Some(session) = SessionReader::read_session_metadata(&path) {
                    sessions.push(session);
                }
            }
        }
        sessions
    }

    /// Extract the first user prompt from a session's JSONL file.
    pub fn extract_first_prompt(&self, session_id: &str) -> Option<String> {
        self.find_jsonl_path(session_id)
            .and_then(|path| SessionReader::extract_first_prompt(&path))
    }

    /// Find a session by ID across all projects.
    pub fn find_session(&self, session_id: &str) -> Option<ClaudeSession> {
        let path = self.find_jsonl_path(session_id)?;
        SessionReader::read_session_metadata(&path)
    }

    /// Find the JSONL file path for a session by scanning project directories.
    pub fn find_jsonl_path(&self, session_id: &str) -> Option<PathBuf> {
        let projects_dir = self.projects_dir();
        if !projects_dir.exists() {
            return None;
        }

        let filename = format!("{}.jsonl", session_id);

        for entry in fs::read_dir(&projects_dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let jsonl_path = path.join(&filename);
                if jsonl_path.exists() {
                    return Some(jsonl_path);
                }
            }
        }

        None
    }
}

impl Default for SessionScanner {
    fn default() -> Self {
        Self::new().expect("HOME environment variable must be set")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_projects_dir() {
        let scanner = SessionScanner::with_dir(PathBuf::from("/tmp/.claude"));
        assert_eq!(scanner.projects_dir(), PathBuf::from("/tmp/.claude/projects"));
    }
}
