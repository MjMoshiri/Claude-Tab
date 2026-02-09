//! JSONL Tailer
//!
//! Watches JSONL session files via OS filesystem events (kqueue/FSEvents).
//! Detects interrupt markers the instant they're written to disk.

use crate::models::SessionMessage;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::{debug, warn};

/// Events detected by tailing a JSONL file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TailEvent {
    Interrupted,
}

/// Shared state between the watcher thread and the async API.
/// Maps jsonl_path → (tab_session_id, last_known_file_size).
type WatchState = Arc<Mutex<HashMap<PathBuf, (String, u64)>>>;

/// Watches JSONL files for changes and emits events when interrupt markers appear.
///
/// Watches `~/.claude/projects/` recursively via OS-level notifications.
/// Only reacts to files registered via `register()`.
pub struct JsonlTailer {
    state: WatchState,
    event_rx: mpsc::UnboundedReceiver<(String, TailEvent)>,
    /// Kept alive to maintain the filesystem watch.
    _watcher: RecommendedWatcher,
}

impl JsonlTailer {
    /// Create a new tailer that watches `~/.claude/projects/` for JSONL changes.
    pub fn new() -> Option<Self> {
        let home = std::env::var("HOME").ok()?;
        let projects_dir = PathBuf::from(&home).join(".claude").join("projects");
        if !projects_dir.exists() {
            return None;
        }

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let state: WatchState = Arc::new(Mutex::new(HashMap::new()));
        let state_ref = state.clone();

        let mut watcher =
            notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
                let event = match res {
                    Ok(e) => e,
                    Err(_) => return,
                };

                if !matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                    return;
                }

                let mut state = state_ref.lock().unwrap();
                for changed_path in &event.paths {
                    let (session_id, last_size) = match state.get(changed_path) {
                        Some((sid, sz)) => (sid.clone(), *sz),
                        None => continue,
                    };

                    let current_size = match std::fs::metadata(changed_path) {
                        Ok(m) => m.len(),
                        Err(_) => continue,
                    };

                    if current_size <= last_size {
                        continue;
                    }

                    if has_interrupt(changed_path, last_size, current_size) {
                        debug!(session_id = %session_id, "Interrupt detected via JSONL watcher");
                        let _ = event_tx.send((session_id.clone(), TailEvent::Interrupted));
                    }

                    // Always advance the cursor
                    if let Some(entry) = state.get_mut(changed_path) {
                        entry.1 = current_size;
                    }
                }
            })
            .ok()?;

        if let Err(e) = watcher.watch(&projects_dir, RecursiveMode::Recursive) {
            warn!(error = %e, "Failed to watch projects directory");
            return None;
        }

        Some(Self {
            state,
            event_rx,
            _watcher: watcher,
        })
    }

    /// Get a shared handle for registering/unregistering sessions from other tasks.
    pub fn share(&self) -> TailerHandle {
        TailerHandle {
            state: self.state.clone(),
        }
    }

    /// Register a session's JSONL file for interrupt detection.
    /// Starts tracking from the current file size (ignores historical interrupts).
    pub fn register(&self, session_id: &str, jsonl_path: PathBuf) {
        let size = std::fs::metadata(&jsonl_path)
            .map(|m| m.len())
            .unwrap_or(0);
        self.state
            .lock()
            .unwrap()
            .insert(jsonl_path, (session_id.to_string(), size));
        debug!(session_id = %session_id, "JSONL watcher: registered");
    }

    /// Unregister a session (stop watching its JSONL file).
    pub fn unregister(&self, session_id: &str) {
        self.state
            .lock()
            .unwrap()
            .retain(|_, (sid, _)| sid != session_id);
    }

    /// Check if a session is registered.
    pub fn is_registered(&self, session_id: &str) -> bool {
        self.state
            .lock()
            .unwrap()
            .values()
            .any(|(sid, _)| sid == session_id)
    }

    /// Receive the next detected event. Awaits until an event arrives.
    pub async fn recv(&mut self) -> Option<(String, TailEvent)> {
        self.event_rx.recv().await
    }
}

/// Shared handle for registering/unregistering sessions from any task.
#[derive(Clone)]
pub struct TailerHandle {
    state: WatchState,
}

impl TailerHandle {
    pub fn register(&self, session_id: &str, jsonl_path: PathBuf) {
        let size = std::fs::metadata(&jsonl_path)
            .map(|m| m.len())
            .unwrap_or(0);
        self.state
            .lock()
            .unwrap()
            .insert(jsonl_path, (session_id.to_string(), size));
        debug!(session_id = %session_id, "JSONL watcher: registered");
    }

    pub fn unregister(&self, session_id: &str) {
        self.state
            .lock()
            .unwrap()
            .retain(|_, (sid, _)| sid != session_id);
    }

    pub fn is_registered(&self, session_id: &str) -> bool {
        self.state
            .lock()
            .unwrap()
            .values()
            .any(|(sid, _)| sid == session_id)
    }
}

/// Check if new bytes in a JSONL file contain an interrupt marker.
fn has_interrupt(path: &Path, from: u64, to: u64) -> bool {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    if file.seek(SeekFrom::Start(from)).is_err() {
        return false;
    }

    let bytes_to_read = ((to - from) as usize).min(262144);
    let mut buf = String::with_capacity(bytes_to_read);
    if file
        .take(bytes_to_read as u64)
        .read_to_string(&mut buf)
        .is_err()
    {
        return false;
    }

    for line in buf.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let msg: SessionMessage = match serde_json::from_str(line) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if msg.message_type == "user" && is_interrupt_content(msg.content()) {
            return true;
        }
    }

    false
}

/// Check if message content is an interrupt marker.
fn is_interrupt_content(content: Option<&serde_json::Value>) -> bool {
    match content {
        Some(serde_json::Value::String(s)) => s.starts_with("[Request interrupted by user"),
        Some(serde_json::Value::Array(arr)) => arr.iter().any(|item| {
            item.get("type").and_then(|t| t.as_str()) == Some("text")
                && item
                    .get("text")
                    .and_then(|t| t.as_str())
                    .map_or(false, |t| t.starts_with("[Request interrupted by user"))
        }),
        _ => false,
    }
}
