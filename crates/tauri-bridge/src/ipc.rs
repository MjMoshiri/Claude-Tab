use base64::Engine;
use claude_tabs_core::config::Config;
use claude_tabs_core::event_bus::EventBus;
use claude_tabs_core::profile::ProfileStore;
use claude_tabs_core::session::SessionStore;
use claude_tabs_core::skills::SkillManager;
use claude_tabs_core::state_machine::StateMachine;
use claude_tabs_pty::{OutputStream, PtyManager};
use claude_tabs_storage::StorageBackend;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tracing::debug;

pub struct AppState {
    pub event_bus: Arc<EventBus>,
    pub config: Arc<Config>,
    pub session_store: Arc<SessionStore>,
    pub pty_manager: Arc<PtyManager>,
    pub output_stream: Arc<OutputStream>,
    pub storage: Arc<dyn StorageBackend>,
    pub profile_store: Arc<ProfileStore>,
    pub state_machine: Arc<StateMachine>,
    pub skill_manager: Arc<SkillManager>,
}

pub struct IpcBridge {
    app_handle: AppHandle,
    output_stream: Arc<OutputStream>,
    event_bus: Arc<EventBus>,
}

impl IpcBridge {
    pub fn new(
        app_handle: AppHandle,
        output_stream: Arc<OutputStream>,
        event_bus: Arc<EventBus>,
    ) -> Self {
        Self {
            app_handle,
            output_stream,
            event_bus,
        }
    }

    pub fn start_forwarding(&self) {
        self.forward_pty_output();
        self.forward_events();
    }

    fn forward_pty_output(&self) {
        let mut receiver = self.output_stream.subscribe();
        let app_handle = self.app_handle.clone();

        tauri::async_runtime::spawn(async move {
            loop {
                match receiver.recv().await {
                    Ok(chunk) => {
                        if chunk.data.is_empty() {
                            let _ = app_handle.emit(
                                "pty-exit",
                                serde_json::json!({ "session_id": chunk.session_id.as_ref() }),
                            );
                        } else {
                            // Use Base64 encoding for binary data - much more efficient than JSON array
                            let encoded = base64::engine::general_purpose::STANDARD.encode(&chunk.data);
                            let payload = serde_json::json!({
                                "session_id": chunk.session_id.as_ref(),
                                "data": encoded,
                                "encoding": "base64",
                            });
                            let _ = app_handle.emit("pty-output", payload);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        debug!(lagged = n, "PTY output receiver lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    fn forward_events(&self) {
        let mut receiver = self.event_bus.receiver();
        let app_handle = self.app_handle.clone();

        tauri::async_runtime::spawn(async move {
            loop {
                match receiver.recv().await {
                    Ok(event) => {
                        let payload = serde_json::json!({
                            "topic": event.topic,
                            "payload": event.payload,
                            "session_id": event.session_id,
                        });
                        let _ = app_handle.emit("core-event", payload);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        debug!(lagged = n, "Event receiver lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }
}
