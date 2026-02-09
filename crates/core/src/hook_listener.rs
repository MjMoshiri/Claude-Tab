use crate::event_bus::EventBus;
use crate::session::SessionStore;
use crate::state_machine::{SessionState, StateMachine};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncReadExt;
use tokio::net::UnixListener;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 600; // 10 minutes

const HOOK_SCRIPT: &str = include_str!("../../../src-tauri/resources/claude-tabs-hook.sh");
const SHELL_INTEGRATION_BASH: &str =
    include_str!("../../../src-tauri/resources/shell-integration.bash");
const SHELL_INTEGRATION_ZSH: &str =
    include_str!("../../../src-tauri/resources/shell-integration.zsh");

#[derive(Debug, Deserialize)]
struct HookMessage {
    session_id: String,
    hook_event_name: String,
    claude_session_id: Option<String>,
    #[serde(default)]
    tool_name: Option<String>,
    #[serde(default)]
    notification_type: Option<String>,
}

enum HookAction {
    Transition(SessionState, &'static str),
    RemoveSession,
    EmitOnly,
    Ignore,
}

pub struct HookListener;

impl HookListener {
    pub fn socket_path() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let pid = std::process::id();
        PathBuf::from(home)
            .join(".claude-tabs")
            .join(format!("hook-{}.sock", pid))
    }

    /// Get the path to the shell integration script for the given shell.
    pub fn shell_integration_path(shell: &str) -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let filename = match shell {
            "zsh" => "shell-integration.zsh",
            _ => "shell-integration.bash",
        };
        PathBuf::from(home).join(".claude-tabs").join(filename)
    }

    /// Remove stale socket files from dead processes.
    pub fn cleanup_stale_sockets() {
        let home = match std::env::var("HOME") {
            Ok(h) => h,
            Err(_) => return,
        };
        let dir = PathBuf::from(home).join(".claude-tabs");
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if let Some(pid_str) = name_str
                .strip_prefix("hook-")
                .and_then(|s| s.strip_suffix(".sock"))
            {
                if let Ok(pid) = pid_str.parse::<u32>() {
                    let alive = unsafe { libc::kill(pid as i32, 0) } == 0;
                    if !alive {
                        let path = entry.path();
                        if std::fs::remove_file(&path).is_ok() {
                            info!(path = %path.display(), "Removed stale hook socket");
                        }
                    }
                }
            }
        }
    }

    pub fn install_hooks() {
        let home = match std::env::var("HOME") {
            Ok(h) => h,
            Err(_) => return,
        };
        let home_path = PathBuf::from(&home);

        // Install hook script
        let hook_dir = home_path.join(".claude-tabs");
        let _ = std::fs::create_dir_all(&hook_dir);
        let hook_script_path = hook_dir.join("claude-tabs-hook.sh");
        if std::fs::write(&hook_script_path, HOOK_SCRIPT).is_err() {
            warn!("Failed to write hook script");
            return;
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                &hook_script_path,
                std::fs::Permissions::from_mode(0o755),
            );
        }

        // Install shell integration scripts
        let bash_integration_path = hook_dir.join("shell-integration.bash");
        if let Err(e) = std::fs::write(&bash_integration_path, SHELL_INTEGRATION_BASH) {
            warn!(error = %e, "Failed to write bash shell integration");
        }

        let zsh_integration_path = hook_dir.join("shell-integration.zsh");
        if let Err(e) = std::fs::write(&zsh_integration_path, SHELL_INTEGRATION_ZSH) {
            warn!(error = %e, "Failed to write zsh shell integration");
        }

        // Update ~/.claude/settings.json
        let claude_dir = home_path.join(".claude");
        let _ = std::fs::create_dir_all(&claude_dir);
        let settings_path = claude_dir.join("settings.json");
        let hook_cmd = hook_script_path.to_string_lossy().to_string();

        let mut settings: serde_json::Value = if settings_path.exists() {
            std::fs::read_to_string(&settings_path)
                .ok()
                .and_then(|c| serde_json::from_str(&c).ok())
                .unwrap_or(serde_json::json!({}))
        } else {
            serde_json::json!({})
        };

        let hook_entry = serde_json::json!([{
            "matcher": "*",
            "hooks": [{ "type": "command", "command": hook_cmd }]
        }]);

        if let Some(obj) = settings.as_object_mut() {
            let hooks = obj.entry("hooks").or_insert(serde_json::json!({}));
            if let Some(hooks_obj) = hooks.as_object_mut() {
                hooks_obj.insert("SessionStart".to_string(), hook_entry.clone());
                hooks_obj.insert("UserPromptSubmit".to_string(), hook_entry.clone());
                hooks_obj.insert("PermissionRequest".to_string(), hook_entry.clone());
                hooks_obj.insert("PreToolUse".to_string(), hook_entry.clone());
                hooks_obj.insert("PostToolUse".to_string(), hook_entry.clone());
                hooks_obj.insert("PostToolUseFailure".to_string(), hook_entry.clone());
                hooks_obj.insert("Notification".to_string(), hook_entry.clone());
                hooks_obj.insert("Stop".to_string(), hook_entry.clone());
                hooks_obj.insert("SessionEnd".to_string(), hook_entry.clone());
                hooks_obj.insert("SubagentStart".to_string(), hook_entry.clone());
                hooks_obj.insert("SubagentStop".to_string(), hook_entry);
            }
        }

        if let Ok(output) = serde_json::to_string_pretty(&settings) {
            let _ = std::fs::write(&settings_path, output);
        }

        info!(path = %settings_path.display(), "Hooks auto-configured");
    }

    pub fn start(
        state_machine: Arc<StateMachine>,
        session_store: Arc<SessionStore>,
        event_bus: Arc<EventBus>,
        idle_timeout_secs: Option<u64>,
    ) {
        Self::install_hooks();
        Self::cleanup_stale_sockets();

        let socket_path = Self::socket_path();
        let idle_timeout =
            Duration::from_secs(idle_timeout_secs.unwrap_or(DEFAULT_IDLE_TIMEOUT_SECS));

        if let Some(parent) = socket_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::remove_file(&socket_path);

        let last_activity: Arc<Mutex<HashMap<String, Instant>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Task 1: Socket listener
        let sm = state_machine.clone();
        let ss = session_store.clone();
        let eb = event_bus.clone();
        let activity = last_activity.clone();
        let path = socket_path.clone();

        tokio::spawn(async move {
            let listener = match UnixListener::bind(&path) {
                Ok(l) => l,
                Err(e) => {
                    error!(path = %path.display(), error = %e, "Failed to bind hook socket");
                    return;
                }
            };

            info!(path = %path.display(), "Hook listener started");

            loop {
                match listener.accept().await {
                    Ok((stream, _)) => {
                        let sm = sm.clone();
                        let ss = ss.clone();
                        let eb = eb.clone();
                        let activity = activity.clone();

                        tokio::spawn(async move {
                            const MAX_MSG_SIZE: usize = 65536; // 64KB limit
                            let mut buf = Vec::with_capacity(4096);
                            if let Err(e) = stream.take(MAX_MSG_SIZE as u64).read_to_end(&mut buf).await {
                                debug!(error = %e, "Failed to read from hook connection");
                                return;
                            }
                            if buf.len() >= MAX_MSG_SIZE {
                                warn!("Hook message exceeded 64KB limit, discarding");
                                return;
                            }
                            let data = String::from_utf8_lossy(&buf);
                            Self::handle_message(&data, &sm, &ss, &eb, &activity).await;
                        });
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to accept hook connection");
                    }
                }
            }
        });

        // Task 2: Idle timeout checker + stale state recovery (every 30s)
        let sm_idle = state_machine;
        let ss_idle = session_store;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;

                let now = Instant::now();
                let timestamps = last_activity.lock().await;
                let timed_out: Vec<String> = timestamps
                    .iter()
                    .filter(|(_, &ts)| now.duration_since(ts) > idle_timeout)
                    .map(|(id, _)| id.clone())
                    .collect();
                drop(timestamps);

                for session_id in timed_out {
                    match sm_idle
                        .transition_session(&session_id, SessionState::Idle, "idle_timeout")
                        .await
                    {
                        Ok(transition) => {
                            let event = crate::Event::new(
                                "session.state_changed",
                                serde_json::json!({
                                    "session_id": session_id,
                                    "from": transition.from,
                                    "to": SessionState::Idle,
                                }),
                            );
                            event_bus.emit(event).await;
                            info!(session_id = %session_id, "Session went idle (timeout)");
                        }
                        Err(e) => {
                            debug!(session_id = %session_id, error = %e, "Idle transition skipped");
                        }
                    }
                    last_activity.lock().await.remove(&session_id);
                }

                // Stale state recovery: running > 5 min with no hook events -> active
                let stale_sessions: Vec<String> = {
                    let activity_map = last_activity.lock().await;
                    let sessions = ss_idle.list().await;
                    let mut stale = Vec::new();
                    for session in &sessions {
                        if session.state == SessionState::Running {
                            let is_stale = activity_map
                                .get(&session.id)
                                .map(|&ts| now.duration_since(ts) > Duration::from_secs(300))
                                .unwrap_or(true);
                            if is_stale {
                                stale.push(session.id.clone());
                            }
                        } else if session.state == SessionState::YourTurn {
                            let long_wait = activity_map
                                .get(&session.id)
                                .map(|&ts| now.duration_since(ts) > Duration::from_secs(1800))
                                .unwrap_or(false);
                            if long_wait {
                                warn!(session_id = %session.id, "Session in your_turn for > 30 minutes");
                            }
                        }
                    }
                    stale
                };

                for session_id in stale_sessions {
                    match sm_idle
                        .transition_session(&session_id, SessionState::Active, "stale_recovery")
                        .await
                    {
                        Ok(transition) => {
                            let event = crate::Event::new(
                                "session.state_changed",
                                serde_json::json!({
                                    "session_id": session_id,
                                    "from": transition.from,
                                    "to": SessionState::Active,
                                }),
                            );
                            event_bus.emit(event).await;
                            warn!(session_id = %session_id, "Stale recovery: running -> active (no hooks for 5 min)");
                        }
                        Err(e) => {
                            debug!(session_id = %session_id, error = %e, "Stale recovery transition skipped");
                        }
                    }
                }
            }
        });
    }

    fn resolve_action(msg: &HookMessage) -> HookAction {
        match msg.hook_event_name.as_str() {
            "SessionStart" => HookAction::Transition(SessionState::Active, "hook.SessionStart"),
            "UserPromptSubmit" => HookAction::Transition(SessionState::Running, "hook.UserPromptSubmit"),
            "PermissionRequest" => HookAction::Transition(SessionState::YourTurn, "hook.PermissionRequest"),
            "PreToolUse" => {
                if msg.tool_name.as_deref() == Some("AskUserQuestion") {
                    HookAction::Transition(SessionState::YourTurn, "hook.PreToolUse.AskUserQuestion")
                } else {
                    HookAction::Ignore
                }
            }
            "Notification" => {
                match msg.notification_type.as_deref() {
                    Some("elicitation_dialog") | Some("permission_prompt") => {
                        HookAction::Transition(SessionState::YourTurn, "hook.Notification.elicitation")
                    }
                    _ => {
                        debug!(notification_type = ?msg.notification_type, "Ignoring notification");
                        HookAction::Ignore
                    }
                }
            }
            "Stop" => HookAction::Transition(SessionState::Active, "hook.Stop"),
            "PostToolUse" => HookAction::Transition(SessionState::Running, "hook.PostToolUse"),
            "PostToolUseFailure" => HookAction::Ignore,
            "SessionEnd" => HookAction::RemoveSession,
            "SubagentStart" => HookAction::Transition(SessionState::Running, "hook.SubagentStart"),
            "SubagentStop" => HookAction::EmitOnly,
            _ => {
                debug!(event = %msg.hook_event_name, "Ignoring unhandled hook event");
                HookAction::Ignore
            }
        }
    }

    async fn handle_message(
        data: &str,
        state_machine: &StateMachine,
        session_store: &SessionStore,
        event_bus: &EventBus,
        last_activity: &Mutex<HashMap<String, Instant>>,
    ) {
        let msg: HookMessage = match serde_json::from_str(data.trim()) {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "Invalid hook message");
                return;
            }
        };

        debug!(
            session_id = %msg.session_id,
            event = %msg.hook_event_name,
            claude_session_id = ?msg.claude_session_id,
            tool_name = ?msg.tool_name,
            "Hook event received"
        );

        // Store claude_session_id in session metadata if present
        if let Some(ref claude_sid) = msg.claude_session_id {
            if !claude_sid.is_empty() {
                // Check if claude_session_id changed (e.g. after /clear)
                let sid_changed = if let Some(session) = session_store.get(&msg.session_id).await {
                    session
                        .metadata
                        .get("claude_session_id")
                        .and_then(|v| v.as_str())
                        .map_or(true, |old| old != claude_sid)
                } else {
                    true
                };

                session_store
                    .set_metadata(
                        &msg.session_id,
                        "claude_session_id",
                        serde_json::Value::String(claude_sid.clone()),
                    )
                    .await;

                // Reset title tracking so the new session's prompt gets picked up
                if sid_changed {
                    session_store
                        .set_metadata(
                            &msg.session_id,
                            "jsonl_title",
                            serde_json::Value::Bool(false),
                        )
                        .await;
                }
            }
        }

        // Update activity timestamp
        last_activity
            .lock()
            .await
            .insert(msg.session_id.clone(), Instant::now());

        // Verify the session exists
        if session_store.get(&msg.session_id).await.is_none() {
            warn!(session_id = %msg.session_id, "Hook event for unknown session");
            return;
        }

        // Mark session as having received user input (for archive gating)
        if msg.hook_event_name == "UserPromptSubmit" {
            session_store
                .set_metadata(&msg.session_id, "had_user_input", serde_json::Value::Bool(true))
                .await;
        }

        // Emit hook-specific event for extensions to react to
        let hook_event = crate::Event::new(
            &format!("hook.{}", msg.hook_event_name),
            serde_json::json!({
                "session_id": msg.session_id,
                "claude_session_id": msg.claude_session_id,
                "tool_name": msg.tool_name,
            }),
        );
        event_bus.emit(hook_event).await;

        // Interrupt detection is handled by the JSONL tailer (polls file changes).
        // The Stop hook just transitions to Active; the tailer will override to
        // Paused if it sees an interrupt marker in the JSONL.
        let action = Self::resolve_action(&msg);

        match action {
            HookAction::Transition(new_state, trigger) => {
                match state_machine
                    .transition_session(&msg.session_id, new_state, trigger)
                    .await
                {
                    Ok(transition) => {
                        let event = crate::Event::new(
                            "session.state_changed",
                            serde_json::json!({
                                "session_id": msg.session_id,
                                "from": transition.from,
                                "to": new_state,
                            }),
                        );
                        event_bus.emit(event).await;

                        info!(
                            session_id = %msg.session_id,
                            from = %transition.from,
                            to = %new_state,
                            trigger = %trigger,
                            "Session state changed via hook"
                        );
                    }
                    Err(e) => {
                        debug!(
                            session_id = %msg.session_id,
                            to = %new_state,
                            trigger = %trigger,
                            error = %e,
                            "Hook state transition failed"
                        );
                    }
                }
            }
            HookAction::RemoveSession => {
                // SessionEnd: remove from activity tracking, emit event
                last_activity.lock().await.remove(&msg.session_id);
                let event = crate::Event::new(
                    "session.ended",
                    serde_json::json!({
                        "session_id": msg.session_id,
                        "claude_session_id": msg.claude_session_id,
                    }),
                );
                event_bus.emit(event).await;
                info!(session_id = %msg.session_id, "Session ended (hook)");
            }
            HookAction::EmitOnly => {
                debug!(session_id = %msg.session_id, event = %msg.hook_event_name, "Hook event emitted (no state change)");
            }
            HookAction::Ignore => {}
        }
    }
}
