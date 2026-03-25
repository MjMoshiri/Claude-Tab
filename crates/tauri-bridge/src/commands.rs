use crate::ipc::AppState;
use claude_tabs_core::hook_listener::HookListener;
use claude_tabs_core::profile::{self, Pack, Profile, ProfileLaunchRequest};
use claude_tabs_core::session::Session;
use claude_tabs_core::profile::SystemPromptEntry;
use claude_tabs_core::skills::{SkillError, SkillInfo};
use claude_tabs_core::state_machine::{SessionState, TransitionError};
use claude_tabs_core::traits::provider::PtySize;
use claude_tabs_core::worktree::{self, WorktreeError, WorktreeInfo};
use claude_tabs_platform_focus::{self as platform_focus, AttentionType};
use claude_tabs_pty::PtyError;
use claude_tabs_storage::{ClaudeSession, DirectoryPreference, SessionFilter, SessionMessage, SessionMetadata, StorageError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::State;
use tracing::{debug, info};

fn policy_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".claude").join("auto-accept-policies")
}

/// Write a per-session policy file to ~/.claude/auto-accept-policies/{session_id}
fn write_policy_file(session_id: &str, policy: &str) -> std::io::Result<()> {
    let dir = policy_dir();
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(session_id), policy)
}

/// Read a per-session policy file from ~/.claude/auto-accept-policies/{session_id}
fn read_policy_file(session_id: &str) -> std::io::Result<String> {
    std::fs::read_to_string(policy_dir().join(session_id))
}

#[derive(Debug, thiserror::Error, Serialize)]
pub enum CommandError {
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    #[error("PTY error: {0}")]
    PtyError(String),
    #[error("Storage error: {0}")]
    StorageError(String),
    #[error("Profile not found: {0}")]
    ProfileNotFound(String),
    #[error("Invalid state: {0}")]
    InvalidState(String),
    #[error("Platform error: {0}")]
    PlatformError(String),
    #[error("{0}")]
    Internal(String),
}

impl From<PtyError> for CommandError {
    fn from(e: PtyError) -> Self {
        match e {
            PtyError::NotFound(id) => CommandError::SessionNotFound(id),
            other => CommandError::PtyError(other.to_string()),
        }
    }
}

impl From<StorageError> for CommandError {
    fn from(e: StorageError) -> Self {
        CommandError::StorageError(e.to_string())
    }
}

impl From<TransitionError> for CommandError {
    fn from(e: TransitionError) -> Self {
        match e {
            TransitionError::SessionNotFound(id) => CommandError::SessionNotFound(id),
            TransitionError::InvalidTransition { from, to, trigger } => {
                CommandError::InvalidState(format!(
                    "Transition from '{}' to '{}' not allowed (trigger: {})", from, to, trigger
                ))
            }
            other => CommandError::InvalidState(other.to_string()),
        }
    }
}

impl From<claude_tabs_platform_focus::FocusError> for CommandError {
    fn from(e: claude_tabs_platform_focus::FocusError) -> Self {
        CommandError::PlatformError(e.to_string())
    }
}

impl From<WorktreeError> for CommandError {
    fn from(e: WorktreeError) -> Self {
        CommandError::Internal(e.to_string())
    }
}

impl From<SkillError> for CommandError {
    fn from(e: SkillError) -> Self {
        CommandError::Internal(e.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub provider_id: String,
    pub state: String,
    pub title: String,
    pub working_directory: Option<String>,
    pub subtitle: Option<String>,
    pub summary: Option<String>,
    pub user_set_title: bool,
    pub generated_title: Option<String>,
    pub hidden: bool,
    pub previous_session_id: Option<String>,
}

impl From<&Session> for SessionInfo {
    fn from(session: &Session) -> Self {
        Self {
            id: session.id.clone(),
            provider_id: session.provider_id.clone(),
            state: session.state.as_str().to_string(),
            title: session.title.clone(),
            working_directory: session.working_directory.clone(),
            subtitle: session
                .metadata
                .get("subtitle")
                .and_then(|v| v.as_str())
                .map(String::from),
            summary: session
                .metadata
                .get("summary")
                .and_then(|v| v.as_str())
                .map(String::from),
            user_set_title: false,
            generated_title: None,
            hidden: false,
            previous_session_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub provider_id: String,
    pub working_directory: Option<String>,
    pub title: Option<String>,
    pub resume_claude_session_id: Option<String>,
    pub fork: Option<bool>,
    pub initial_prompt: Option<String>,
    pub allowed_tools: Option<Vec<String>>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub system_prompt_file: Option<String>,
    #[serde(default)]
    pub metadata: Option<HashMap<String, serde_json::Value>>,
    #[serde(default)]
    pub dangerously_skip_permissions: bool,
}

/// Resolve PATH from the user's login shell so app bundles (macOS) can find
/// CLIs that are only in shell-configured paths (e.g. /opt/homebrew/bin).
fn get_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    std::process::Command::new(&shell)
        .args(["-l", "-c", "echo $PATH"])
        .output()
        .ok()
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        })
}

#[tauri::command]
pub async fn create_session(
    state: State<'_, AppState>,
    request: CreateSessionRequest,
) -> Result<SessionInfo, CommandError> {
    info!(provider_id = %request.provider_id, "Creating session");

    let mut session = Session::new(&request.provider_id);
    if let Some(title) = &request.title {
        session.title = title.clone();
    }
    if let Some(dir) = &request.working_directory {
        session.working_directory = Some(dir.clone());
    }

    let session_id = session.id.clone();

    let mut env = std::collections::HashMap::new();
    env.insert("TERM".to_string(), "xterm-256color".to_string());
    env.insert("CLAUDE_TABS_SESSION_ID".to_string(), session_id.clone());

    // Inject shell PATH so app bundles (macOS) can find CLIs like `claude`
    if let Some(shell_path) = get_shell_path() {
        env.insert("PATH".to_string(), shell_path);
    }

    env.insert(
        "CLAUDE_TABS_SOCKET".to_string(),
        HookListener::socket_path().to_string_lossy().to_string(),
    );

    // For bash sessions, set BASH_ENV to source shell integration
    if request.provider_id != "claude-code" {
        env.insert(
            "BASH_ENV".to_string(),
            HookListener::shell_integration_path("bash")
                .to_string_lossy()
                .to_string(),
        );
    }

    // Auto-accept: inject env vars when enabled and write per-session policy file
    if let Some(serde_json::Value::Bool(true)) = state.config.get("autoAccept.enabled").await {
        let default_policy = state.config.get("autoAccept.defaultPolicy").await
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        if !default_policy.is_empty() {
            env.insert("AUTO_ACCEPT_POLICY".to_string(), default_policy.clone());
        }
        // Always create policy file (empty if no default) so the hook has a file to read
        let _ = write_policy_file(&session_id, &default_policy);
        if let Some(serde_json::Value::String(model)) = state.config.get("autoAccept.model").await {
            env.insert("AUTO_ACCEPT_MODEL".to_string(), model);
        }
        if let Some(serde_json::Value::String(mode)) = state.config.get("autoAccept.mode").await {
            env.insert("AUTO_ACCEPT_MODE".to_string(), mode);
        }
    }

    // Handle system_prompt_file: read content and set as system_prompt
    let mut request = request;
    if request.system_prompt.is_none() {
        if let Some(ref file_name) = request.system_prompt_file {
            match profile::read_system_prompt_content(file_name) {
                Ok(content) => {
                    request.system_prompt = Some(content);
                }
                Err(e) => {
                    debug!(error = %e, "Failed to read system prompt file");
                }
            }
        }
    }

    let size = PtySize { rows: 24, cols: 80 };

    let command = if request.provider_id == "claude-code" {
        "claude"
    } else {
        "bash"
    };

    let args: Vec<String> = if request.provider_id == "claude-code" {
        let mut a = Vec::new();
        if let Some(ref id) = request.resume_claude_session_id {
            a.push("--resume".to_string());
            a.push(id.clone());
            if request.fork.unwrap_or(false) {
                a.push("--fork-session".to_string());
            }
        }
        if let Some(ref tools) = request.allowed_tools {
            if !tools.is_empty() {
                a.push("--allowedTools".to_string());
                a.push(tools.join(","));
            }
        }
        if let Some(ref model) = request.model {
            a.push("--model".to_string());
            a.push(model.clone());
        }
        if let Some(ref sys_prompt) = request.system_prompt {
            a.push("--append-system-prompt".to_string());
            a.push(sys_prompt.clone());
        }
        if request.dangerously_skip_permissions {
            a.push("--dangerously-skip-permissions".to_string());
        }
        // Add prompt as positional arg (interactive mode) instead of -p (print mode)
        if request.resume_claude_session_id.is_none() {
            if let Some(ref prompt) = request.initial_prompt {
                a.push(prompt.clone());
            }
        }
        a
    } else {
        Vec::new()
    };

    let reader = state
        .pty_manager
        .spawn(
            &session_id,
            command,
            &args,
            request.working_directory.as_deref(),
            &env,
            size,
        )?;

    state.output_stream.start_reading(session_id.clone(), reader);
    state.session_store.add(session.clone()).await;
    state.session_store.set_active(Some(session_id.clone())).await;

    // Apply metadata entries if provided
    if let Some(metadata) = &request.metadata {
        for (key, value) in metadata {
            state.session_store.set_metadata(&session_id, key, value.clone()).await;
        }
        // Set subtitle from worktree branch if present
        if let Some(branch) = metadata.get("worktree_branch").and_then(|v| v.as_str()) {
            state.session_store.set_metadata(
                &session_id,
                "subtitle",
                serde_json::Value::String(format!("\u{2387} {}", branch)),
            ).await;
        }
    }

    let event = claude_tabs_core::Event::new(
        "session.created",
        serde_json::json!({
            "session_id": session_id,
            "provider_id": request.provider_id,
        }),
    );
    state.event_bus.emit(event).await;

    // Re-read session to include metadata in response
    let final_session = state.session_store.get(&session_id).await.unwrap_or(session);
    Ok(SessionInfo::from(&final_session))
}

#[tauri::command]
pub async fn close_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), CommandError> {
    info!(session_id = %session_id, "Closing session");

    // Check for worktree metadata before removing session
    let worktree_info = if let Some(session) = state.session_store.get(&session_id).await {
        let wt_path = session.metadata.get("worktree_path").and_then(|v| v.as_str()).map(String::from);
        let wt_branch = session.metadata.get("worktree_branch").and_then(|v| v.as_str()).map(String::from);
        wt_path.zip(wt_branch)
    } else {
        None
    };

    // Remove from store
    state.session_store.remove(&session_id).await;

    state.pty_manager.close(&session_id)?;

    let sessions = state.session_store.list().await;
    let new_active = sessions.first().map(|s| s.id.clone());
    state.session_store.set_active(new_active).await;

    // Emit worktree cleanup event if session had a worktree
    if let Some((wt_path, wt_branch)) = worktree_info {
        let cleanup_event = claude_tabs_core::Event::new(
            "session.worktree_cleanup",
            serde_json::json!({
                "session_id": session_id,
                "worktree_path": wt_path,
                "worktree_branch": wt_branch,
            }),
        );
        state.event_bus.emit(cleanup_event).await;
    }

    let event = claude_tabs_core::Event::new(
        "session.closed",
        serde_json::json!({ "session_id": session_id }),
    );
    state.event_bus.emit(event).await;

    Ok(())
}

#[tauri::command]
pub async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionInfo>, CommandError> {
    let sessions = state.session_store.list().await;
    Ok(sessions.iter().map(SessionInfo::from).collect())
}

#[tauri::command]
pub async fn get_active_session(state: State<'_, AppState>) -> Result<Option<String>, CommandError> {
    Ok(state.session_store.get_active().await)
}

#[tauri::command]
pub async fn set_active_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), CommandError> {
    debug!(session_id = %session_id, "Setting active session");
    state.session_store.set_active(Some(session_id.clone())).await;

    // Emit active_changed event for UI sync (sidebar, declined sessions, etc.)
    let active_event = claude_tabs_core::Event::new(
        "session.active_changed",
        serde_json::json!({ "session_id": session_id }),
    );
    state.event_bus.emit(active_event).await;

    // Transition idle/completed → active on focus. Don't touch running, your_turn, paused, or active sessions.
    if let Some(session) = state.session_store.get(&session_id).await {
        if matches!(session.state, SessionState::Idle | SessionState::Completed) {
            if let Ok(transition) = state
                .state_machine
                .transition_session(&session_id, SessionState::Active, "user.focus")
                .await
            {
                let event = claude_tabs_core::Event::new(
                    "session.state_changed",
                    serde_json::json!({
                        "session_id": session_id,
                        "from": transition.from.as_str(),
                        "to": "active",
                    }),
                );
                state.event_bus.emit(event).await;
            }
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn rename_session(
    state: State<'_, AppState>,
    session_id: String,
    title: String,
) -> Result<(), CommandError> {
    info!(session_id = %session_id, title = %title, "Renaming session");
    let success = state.session_store.rename(&session_id, &title).await;
    if !success {
        return Err(CommandError::SessionNotFound(session_id.clone()));
    }

    // Persist custom title to DB if session has a claude_session_id
    if let Some(session) = state.session_store.get(&session_id).await {
        if let Some(claude_sid) = session.metadata.get("claude_session_id").and_then(|v| v.as_str()) {
            let existing = state.storage.get_session_metadata(claude_sid).await.ok().flatten();
            let meta = if let Some(mut m) = existing {
                m.custom_title = Some(title.clone());
                m.user_set_title = true;
                m
            } else {
                SessionMetadata {
                    claude_session_id: claude_sid.to_string(),
                    project_path: session.working_directory.clone().unwrap_or_default(),
                    custom_title: Some(title.clone()),
                    user_set_title: true,
                    generated_title: None,
                    hidden: false,
                    previous_session_id: None,
                    last_known_state: None,
                    last_state_change_at: None,
                    created_at: String::new(),
                    updated_at: String::new(),
                }
            };
            let _ = state.storage.upsert_session_metadata(&meta).await;
        }
    }

    let event = claude_tabs_core::Event::new(
        "session.renamed",
        serde_json::json!({ "session_id": session_id, "title": title, "source": "user" }),
    );
    state.event_bus.emit(event).await;
    Ok(())
}

#[tauri::command]
pub async fn write_to_pty(
    state: State<'_, AppState>,
    session_id: String,
    data: Vec<u8>,
) -> Result<(), CommandError> {
    state
        .pty_manager
        .write_data(&session_id, &data)?;

    // Enter key (\r) while in your_turn/completed → transition to running.
    // This covers the gap where Claude asks for permission and the user presses
    // Enter to respond, but no hook fires until the tool finishes.
    if data.contains(&b'\r') {
        let sm = state.state_machine.clone();
        let ss = state.session_store.clone();
        let eb = state.event_bus.clone();
        let sid = session_id;
        tokio::spawn(async move {
            if let Some(session) = ss.get(&sid).await {
                if matches!(session.state, SessionState::YourTurn | SessionState::Completed) {
                    if let Ok(transition) = sm
                        .transition_session(&sid, SessionState::Running, "terminal.enter")
                        .await
                    {
                        let event = claude_tabs_core::Event::new(
                            "session.state_changed",
                            serde_json::json!({
                                "session_id": sid,
                                "from": transition.from.as_str(),
                                "to": "running",
                            }),
                        );
                        eb.emit(event).await;
                    }
                }
            }
        });
    }

    Ok(())
}

#[tauri::command]
pub fn resize_pty(
    state: State<'_, AppState>,
    session_id: String,
    rows: u16,
    cols: u16,
) -> Result<(), CommandError> {
    let size = PtySize { rows, cols };
    state
        .pty_manager
        .resize(&session_id, size)?;
    Ok(())
}

/// Submit user input to the PTY with a trailing newline.
/// This is used by the hybrid terminal's input area.
#[tauri::command]
pub fn submit_input(
    state: State<'_, AppState>,
    session_id: String,
    input: String,
) -> Result<(), CommandError> {
    let data = format!("{}\n", input);
    state
        .pty_manager
        .write_data(&session_id, data.as_bytes())?;
    Ok(())
}

#[tauri::command]
pub async fn get_config_value(
    state: State<'_, AppState>,
    key: String,
) -> Result<Option<serde_json::Value>, CommandError> {
    Ok(state.config.get(&key).await)
}

#[tauri::command]
pub async fn set_config_value(
    state: State<'_, AppState>,
    key: String,
    value: serde_json::Value,
) -> Result<(), CommandError> {
    state
        .config
        .set_value(&key, value, claude_tabs_core::ConfigLayer::Runtime)
        .await;
    Ok(())
}

#[tauri::command]
pub async fn setup_hooks() -> Result<String, CommandError> {
    HookListener::install_hooks();
    Ok("Hooks installed".to_string())
}

// ============================================================================
// Claude Code Session History Commands
// ============================================================================

#[tauri::command]
pub async fn list_claude_sessions(
    state: State<'_, AppState>,
    filter: Option<SessionFilter>,
) -> Result<Vec<ClaudeSession>, CommandError> {
    let filter = filter.unwrap_or(SessionFilter {
        limit: 100,
        ..Default::default()
    });
    Ok(state.storage.list_sessions(filter).await?)
}

#[tauri::command]
pub async fn get_claude_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Option<ClaudeSession>, CommandError> {
    Ok(state.storage.get_session(&session_id).await?)
}

#[tauri::command]
pub async fn get_session_content(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<SessionMessage>, CommandError> {
    Ok(state.storage.get_session_content(&session_id).await?)
}

#[tauri::command]
pub async fn set_directory_preference(
    state: State<'_, AppState>,
    project_path: String,
    pinned: Option<bool>,
    hidden: Option<bool>,
    display_name: Option<String>,
) -> Result<(), CommandError> {
    // Get existing preference or create new one
    let prefs = state.storage.get_directory_preferences().await?;
    let existing = prefs.iter().find(|p| p.project_path == project_path);

    let pref = DirectoryPreference {
        project_path: project_path.clone(),
        pinned: pinned.unwrap_or_else(|| existing.map(|e| e.pinned).unwrap_or(false)),
        hidden: hidden.unwrap_or_else(|| existing.map(|e| e.hidden).unwrap_or(false)),
        display_name: display_name.or_else(|| existing.and_then(|e| e.display_name.clone())),
    };

    Ok(state.storage.set_directory_preference(pref).await?)
}

#[tauri::command]
pub async fn get_directory_preferences(
    state: State<'_, AppState>,
) -> Result<Vec<DirectoryPreference>, CommandError> {
    Ok(state.storage.get_directory_preferences().await?)
}

#[tauri::command]
pub async fn remove_directory_preference(
    state: State<'_, AppState>,
    project_path: String,
) -> Result<(), CommandError> {
    Ok(state.storage.remove_directory_preference(&project_path).await?)
}

#[tauri::command]
pub async fn delete_history_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), CommandError> {
    info!(session_id = %session_id, "Deleting history session");

    // Get session to find the jsonl path
    let session = state.storage.get_session(&session_id).await?;

    // Delete the file if it exists
    if let Some(s) = session {
        let path = std::path::Path::new(&s.jsonl_path);
        if path.exists() {
            std::fs::remove_file(path)
                .map_err(|e| CommandError::Internal(e.to_string()))?;
            info!(path = %s.jsonl_path, "Deleted session file");
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn delete_project_sessions(
    state: State<'_, AppState>,
    project_path: String,
) -> Result<u32, CommandError> {
    info!(project_path = %project_path, "Deleting all sessions for project");

    // Get all sessions for this project
    let filter = claude_tabs_storage::SessionFilter {
        project_path: Some(project_path.clone()),
        limit: 10000,
        ..Default::default()
    };
    let sessions = state.storage.list_sessions(filter).await?;

    let mut deleted_count = 0u32;
    for session in &sessions {
        // Delete file
        let path = std::path::Path::new(&session.jsonl_path);
        if path.exists() {
            if std::fs::remove_file(path).is_ok() {
                deleted_count += 1;
            } else {
                debug!(path = %session.jsonl_path, "Failed to delete session file");
            }
        }
    }

    info!(project_path = %project_path, deleted_count = deleted_count, "Deleted project sessions");
    Ok(deleted_count)
}

#[tauri::command]
pub async fn resume_session(
    state: State<'_, AppState>,
    claude_session_id: String,
) -> Result<SessionInfo, CommandError> {
    let session = state
        .storage
        .get_session(&claude_session_id)
        .await?
        .ok_or_else(|| CommandError::SessionNotFound(claude_session_id.clone()))?;

    let title = session.first_prompt.as_deref()
        .map(|p| if p.len() > 80 { format!("{}...", &p[..77]) } else { p.to_string() })
        .unwrap_or_else(|| format!("Session {}", &claude_session_id[..8.min(claude_session_id.len())]));

    let request = CreateSessionRequest {
        provider_id: "claude-code".to_string(),
        working_directory: Some(session.project_path.clone()),
        title: Some(title),
        resume_claude_session_id: Some(claude_session_id.clone()),
        fork: None,
        initial_prompt: None,
        allowed_tools: None,
        model: None,
        system_prompt: None,
        system_prompt_file: None,
        metadata: None,
        dangerously_skip_permissions: false,
    };

    let result = create_session(state.clone(), request).await?;

    state
        .session_store
        .set_metadata(
            &result.id,
            "claude_session_id",
            serde_json::Value::String(claude_session_id),
        )
        .await;

    Ok(result)
}

#[tauri::command]
pub async fn fork_session(
    state: State<'_, AppState>,
    claude_session_id: String,
) -> Result<SessionInfo, CommandError> {
    let session = state
        .storage
        .get_session(&claude_session_id)
        .await?
        .ok_or_else(|| CommandError::SessionNotFound(claude_session_id.clone()))?;

    let base_title = session.first_prompt.as_deref()
        .map(|p| if p.len() > 80 { format!("{}...", &p[..77]) } else { p.to_string() })
        .unwrap_or_else(|| format!("Session {}", &claude_session_id[..8.min(claude_session_id.len())]));
    let title = format!("{} (fork)", base_title);

    let request = CreateSessionRequest {
        provider_id: "claude-code".to_string(),
        working_directory: Some(session.project_path.clone()),
        title: Some(title),
        resume_claude_session_id: Some(claude_session_id),
        fork: Some(true),
        initial_prompt: None,
        allowed_tools: None,
        model: None,
        system_prompt: None,
        system_prompt_file: None,
        metadata: None,
        dangerously_skip_permissions: false,
    };

    create_session(state, request).await
}

#[tauri::command]
pub async fn fork_active_session(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<SessionInfo, CommandError> {
    let session = state.session_store.get(&session_id).await
        .ok_or_else(|| CommandError::SessionNotFound(session_id.clone()))?;
    let claude_session_id = session.metadata
        .get("claude_session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| CommandError::Internal("No Claude session ID available for fork".to_string()))?
        .to_string();
    let title = format!("{} (fork)", session.title);
    let request = CreateSessionRequest {
        provider_id: session.provider_id.clone(),
        working_directory: session.working_directory.clone(),
        title: Some(title),
        resume_claude_session_id: Some(claude_session_id),
        fork: Some(true),
        initial_prompt: None,
        allowed_tools: None,
        model: None,
        system_prompt: None,
        system_prompt_file: None,
        metadata: None,
        dangerously_skip_permissions: false,
    };
    create_session(state, request).await
}

// --- Profile commands ---

#[tauri::command]
pub async fn list_profiles(state: State<'_, AppState>) -> Result<Vec<Profile>, CommandError> {
    Ok(state.profile_store.list().await)
}

#[tauri::command]
pub async fn get_profile(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<Profile, CommandError> {
    state
        .profile_store
        .get(&profile_id)
        .await
        .ok_or_else(|| CommandError::ProfileNotFound(profile_id))
}

#[tauri::command]
pub async fn save_profile(
    state: State<'_, AppState>,
    profile: Profile,
) -> Result<(), CommandError> {
    state.profile_store.save(profile).await.map_err(|e| CommandError::Internal(e))
}

#[tauri::command]
pub async fn delete_profile(
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<(), CommandError> {
    state.profile_store.delete(&profile_id).await.map_err(|e| CommandError::Internal(e))
}

#[tauri::command]
pub async fn launch_profile(
    state: State<'_, AppState>,
    request: ProfileLaunchRequest,
) -> Result<SessionInfo, CommandError> {
    let profile = state
        .profile_store
        .get(&request.profile_id)
        .await
        .ok_or_else(|| CommandError::ProfileNotFound(request.profile_id.clone()))?;

    // Resolve working directory
    let working_directory = match &profile.working_directory {
        Some(claude_tabs_core::profile::WorkingDirConfig::Fixed { path }) => Some(path.clone()),
        Some(claude_tabs_core::profile::WorkingDirConfig::FromInput { key }) => {
            request.input_values.get(key).cloned()
        }
        _ => request.working_directory.clone(),
    };

    // Resolve prompt template
    let initial_prompt = profile.prompt_template.as_ref().map(|template| {
        state.profile_store.resolve_prompt(template, &request.input_values)
    });

    // Handle system prompt file from profile
    let system_prompt = if profile.system_prompt.is_some() {
        profile.system_prompt.clone()
    } else if let Some(ref file_name) = profile.system_prompt_file {
        profile::read_system_prompt_content(file_name).ok()
    } else {
        None
    };

    // Sync skills before creating session
    if let Some(ref skills) = profile.skills {
        if !skills.is_empty() {
            state.skill_manager.sync_skills(skills)?;
        }
    }

    let title = profile.name.clone();

    let create_request = CreateSessionRequest {
        provider_id: "claude-code".to_string(),
        working_directory,
        title: Some(title),
        resume_claude_session_id: None,
        fork: None,
        initial_prompt,
        allowed_tools: profile.allowed_tools.clone(),
        model: profile.model.clone(),
        system_prompt,
        system_prompt_file: None, // Already resolved above
        metadata: None,
        dangerously_skip_permissions: profile.dangerously_skip_permissions,
    };

    let result = create_session(state.clone(), create_request).await?;

    // Store profile metadata on the session
    state
        .session_store
        .set_metadata(
            &result.id,
            "profile_id",
            serde_json::Value::String(profile.id.clone()),
        )
        .await;

    // Apply profile's auto-accept policy if set
    if let Some(ref policy) = profile.auto_accept_policy {
        if !policy.is_empty() {
            let _ = write_policy_file(&result.id, policy);
        }
    }

    Ok(result)
}

// --- Pack commands ---

#[tauri::command]
pub async fn list_packs(state: State<'_, AppState>) -> Result<Vec<Pack>, CommandError> {
    Ok(state.pack_store.list().await)
}

#[tauri::command]
pub async fn save_pack(
    state: State<'_, AppState>,
    pack: Pack,
) -> Result<(), CommandError> {
    state.pack_store.save(pack).await.map_err(|e| CommandError::Internal(e))
}

#[tauri::command]
pub async fn delete_pack(
    state: State<'_, AppState>,
    pack_id: String,
) -> Result<(), CommandError> {
    state.pack_store.delete(&pack_id).await.map_err(|e| CommandError::Internal(e))
}

// --- Session policy commands ---

#[tauri::command]
pub async fn get_session_policy(session_id: String) -> Result<Option<String>, CommandError> {
    match read_policy_file(&session_id) {
        Ok(content) => Ok(Some(content)),
        Err(_) => Ok(None),
    }
}

#[tauri::command]
pub async fn set_session_policy(session_id: String, policy: String) -> Result<(), CommandError> {
    write_policy_file(&session_id, &policy).map_err(|e| CommandError::Internal(e.to_string()))
}

// ============================================================================
// Window Focus Commands (Platform-specific)
// ============================================================================

/// Focus the application window using native platform APIs.
/// This bypasses Tauri's buggy setFocus() on macOS.
#[tauri::command]
pub fn focus_window() -> Result<(), CommandError> {
    debug!("focus_window command called");
    Ok(platform_focus::focus_window()?)
}

/// Request user attention (dock bounce on macOS, taskbar flash on Windows).
///
/// # Arguments
/// * `critical` - If true, uses critical/persistent attention. If false, uses informational/brief.
#[tauri::command]
pub fn request_attention(critical: bool) -> Result<(), CommandError> {
    debug!(critical = critical, "request_attention command called");
    let attention_type = if critical {
        AttentionType::Critical
    } else {
        AttentionType::Informational
    };
    Ok(platform_focus::request_attention(attention_type)?)
}

/// Check if the application is currently the frontmost/active app.
#[tauri::command]
pub fn is_app_active() -> Result<bool, CommandError> {
    Ok(platform_focus::is_app_active()?)
}

/// Manually set a session's state.
/// Used for manual state changes like marking a session as idle.
#[tauri::command]
pub async fn set_session_state(
    state: State<'_, AppState>,
    session_id: String,
    new_state: String,
) -> Result<(), CommandError> {
    info!(session_id = %session_id, new_state = %new_state, "Manually setting session state");

    let target: SessionState = serde_json::from_value(serde_json::Value::String(new_state.clone()))
        .map_err(|_| CommandError::InvalidState(format!("Unknown state: {}", new_state)))?;

    let transition = state
        .state_machine
        .transition_session(&session_id, target, "manual")
        .await?;

    let event = claude_tabs_core::Event::new(
        "session.state_changed",
        serde_json::json!({
            "session_id": session_id,
            "from": transition.from.as_str(),
            "to": target.as_str(),
            "manual": true,
        }),
    );
    state.event_bus.emit(event).await;

    Ok(())
}

#[tauri::command]
pub async fn set_session_hidden(
    state: State<'_, AppState>,
    session_id: String,
    hidden: bool,
) -> Result<(), CommandError> {
    state.storage.set_session_hidden(&session_id, hidden).await?;
    Ok(())
}

#[tauri::command]
pub async fn get_session_chain(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<SessionMetadata>, CommandError> {
    Ok(state.storage.get_session_chain(&session_id).await?)
}

#[tauri::command]
pub async fn trigger_title_generation(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Option<String>, CommandError> {
    // Check if user already set title
    let meta = state.storage.get_session_metadata(&session_id).await?;
    if let Some(ref m) = meta {
        if m.user_set_title { return Ok(None); }
        if m.generated_title.is_some() { return Ok(m.generated_title.clone()); }
    }

    // Get messages and check count >= 2
    let messages = state.storage.get_session_content(&session_id).await?;
    let user_msgs: Vec<_> = messages.iter().filter(|m| m.message_type == "user").collect();
    if user_msgs.len() < 2 { return Ok(None); }

    // Build prompt (actual API call would go here in the future)
    let prompt = claude_tabs_core::generate_title_prompt(&messages);
    // For now, just return the prompt - actual Haiku call TBD
    let _ = prompt;
    Ok(None)
}

// ============================================================================
// Git Worktree Commands
// ============================================================================

#[tauri::command]
pub fn check_git_repo(path: String) -> Result<bool, CommandError> {
    Ok(worktree::is_git_repo(&path))
}

#[tauri::command]
pub fn create_worktree(
    repo_path: String,
    branch_name: Option<String>,
) -> Result<WorktreeInfo, CommandError> {
    Ok(worktree::create_worktree(&repo_path, branch_name.as_deref())?)
}

#[tauri::command]
pub fn remove_worktree(worktree_path: String) -> Result<(), CommandError> {
    Ok(worktree::remove_worktree(&worktree_path)?)
}

// ============================================================================
// Skill Management Commands
// ============================================================================

#[tauri::command]
pub fn list_available_skills(
    state: State<'_, AppState>,
) -> Result<Vec<SkillInfo>, CommandError> {
    Ok(state.skill_manager.list_available_skills()?)
}

#[tauri::command]
pub fn sync_skills(
    state: State<'_, AppState>,
    skills: Vec<String>,
) -> Result<(), CommandError> {
    Ok(state.skill_manager.sync_skills(&skills)?)
}

// ============================================================================
// System Prompt Commands
// ============================================================================

#[tauri::command]
pub fn list_system_prompts() -> Result<Vec<SystemPromptEntry>, CommandError> {
    Ok(profile::list_system_prompts())
}

#[tauri::command]
pub fn read_system_prompt(name: String) -> Result<String, CommandError> {
    profile::read_system_prompt_content(&name)
        .map_err(CommandError::Internal)
}

#[tauri::command]
pub fn save_system_prompt(name: String, content: String) -> Result<(), CommandError> {
    profile::save_system_prompt(&name, &content)
        .map_err(CommandError::Internal)
}

#[tauri::command]
pub fn delete_system_prompt(name: String) -> Result<(), CommandError> {
    profile::delete_system_prompt(&name)
        .map_err(CommandError::Internal)
}

