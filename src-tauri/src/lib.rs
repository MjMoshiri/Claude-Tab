use tauri::Manager;
use claude_tabs_core::config::Config;
use claude_tabs_core::event_bus::EventBus;
use claude_tabs_core::hook_listener::HookListener;
use claude_tabs_core::profile::{ProfileStore, PackStore};
use claude_tabs_core::session::SessionStore;
use claude_tabs_core::skills::SkillManager;
use claude_tabs_core::state_machine::{SessionState, StateMachine};
use claude_tabs_ext_telegram_bot::PairingState;
use claude_tabs_pty::{OutputStream, PtyManager};
use claude_tabs_storage::{SessionScanner, SqliteBackend};
use claude_tabs_tauri_bridge::commands;
use claude_tabs_tauri_bridge::ipc::{AppState, IpcBridge};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};
use tracing_subscriber::EnvFilter;

// --- Telegram bot Tauri commands ---

struct TelegramState {
    pairing: Arc<RwLock<PairingState>>,
    pending_code: Arc<RwLock<Option<claude_tabs_ext_telegram_bot::PendingCode>>>,
}

#[tauri::command]
async fn telegram_generate_code(
    state: tauri::State<'_, TelegramState>,
) -> Result<String, String> {
    let code = claude_tabs_ext_telegram_bot::generate_pairing_code(state.pending_code.clone()).await;
    Ok(code)
}

#[tauri::command]
async fn telegram_get_status(
    state: tauri::State<'_, TelegramState>,
) -> Result<PairingState, String> {
    let pairing = state.pairing.read().await;
    Ok(pairing.clone())
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn telegram_disconnect(
    state: tauri::State<'_, TelegramState>,
) -> Result<(), String> {
    claude_tabs_ext_telegram_bot::disconnect(state.pairing.clone()).await;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Starting Claude Tabs");

    let event_bus = Arc::new(EventBus::new(2048));
    let config = Arc::new(Config::new());

    // Set user config file path and load persisted settings
    {
        let config_clone = config.clone();
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        let config_path = std::path::PathBuf::from(format!("{}/.claude-tabs/config.toml", home));
        tauri::async_runtime::block_on(async {
            config_clone.set_user_file_path(config_path.clone()).await;
            if config_path.exists() {
                if let Err(e) = config_clone.load_from_file(&config_path, claude_tabs_core::config::ConfigLayer::User).await {
                    warn!("Failed to load user config: {}", e);
                }
            }
        });
    }
    let session_store = Arc::new(SessionStore::new());
    let pty_manager = Arc::new(PtyManager::new());
    let output_stream = Arc::new(OutputStream::new(512));
    let state_machine = Arc::new(StateMachine::new(session_store.clone()));

    let storage: Arc<dyn claude_tabs_storage::StorageBackend> =
        match SqliteBackend::new("~/.claude-tabs/archive.db") {
            Ok(backend) => Arc::new(backend),
            Err(e) => {
                panic!("Storage initialization failed: {}", e);
            }
        };

    let profile_store = Arc::new(ProfileStore::new());
    let pack_store = Arc::new(PackStore::new());
    let skill_manager = Arc::new(SkillManager::new());

    let app_state = AppState {
        event_bus: event_bus.clone(),
        config: config.clone(),
        session_store: session_store.clone(),
        pty_manager: pty_manager.clone(),
        output_stream: output_stream.clone(),
        storage: storage.clone(),
        profile_store: profile_store.clone(),
        pack_store: pack_store.clone(),
        state_machine: state_machine.clone(),
        skill_manager: skill_manager.clone(),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(app_state)
        .manage(TelegramState {
            pairing: Arc::new(tokio::sync::RwLock::new(claude_tabs_ext_telegram_bot::get_pairing_status())),
            pending_code: Arc::new(tokio::sync::RwLock::new(None)),
        })
        .invoke_handler(tauri::generate_handler![
            commands::create_session,
            commands::close_session,
            commands::list_sessions,
            commands::get_active_session,
            commands::set_active_session,
            commands::rename_session,
            commands::write_to_pty,
            commands::resize_pty,
            commands::submit_input,
            commands::get_config_value,
            commands::set_config_value,
            commands::save_config,
            commands::setup_hooks,
            // Claude Code session history
            commands::list_claude_sessions,
            commands::get_claude_session,
            commands::get_session_content,
            commands::set_directory_preference,
            commands::get_directory_preferences,
            commands::remove_directory_preference,
            commands::delete_history_session,
            commands::delete_project_sessions,
            commands::resume_session,
            commands::fork_session,
            commands::fork_active_session,
            // Profiles
            commands::list_profiles,
            commands::get_profile,
            commands::save_profile,
            commands::delete_profile,
            commands::launch_profile,
            // Packs
            commands::list_packs,
            commands::save_pack,
            commands::delete_pack,
            // Window focus (platform-specific)
            commands::focus_window,
            commands::request_attention,
            commands::is_app_active,
            // Session state management
            commands::set_session_state,
            commands::set_session_hidden,
            commands::get_session_chain,
            commands::trigger_title_generation,
            // Git worktree
            commands::check_git_repo,
            commands::create_worktree,
            commands::remove_worktree,
            // Skill management
            commands::list_available_skills,
            commands::sync_skills,
            // System prompt discovery
            commands::list_system_prompts,
            commands::read_system_prompt,
            commands::save_system_prompt,
            commands::delete_system_prompt,
            // Session policy
            commands::get_session_policy,
            commands::set_session_policy,
            // Telegram bot
            open_url,
            telegram_generate_code,
            telegram_get_status,
            telegram_disconnect,
        ])
        .setup(move |app| {
            let app_handle = app.handle().clone();

            let eb = event_bus.clone();
            let os = output_stream.clone();
            let ss = session_store.clone();

            let ps = profile_store.clone();
            let pks = pack_store.clone();
            tauri::async_runtime::spawn(async move {
                ps.init().await;
                pks.init().await;
                info!("Profiles and packs loaded");
            });

            // Start Telegram bot via tauri::async_runtime::spawn
            // (raw tokio::spawn panics here since setup runs on the main thread)
            {
                let tg_state: tauri::State<'_, TelegramState> = app.state();
                let bot_future = claude_tabs_ext_telegram_bot::create_bot_future(
                    config.clone(),
                    profile_store.clone(),
                    pack_store.clone(),
                    session_store.clone(),
                    pty_manager.clone(),
                    output_stream.clone(),
                    event_bus.clone(),
                    skill_manager.clone(),
                    tg_state.pairing.clone(),
                    tg_state.pending_code.clone(),
                );
                tauri::async_runtime::spawn(bot_future);
                info!("Telegram bot extension started");
            }

            let bridge = IpcBridge::new(app_handle, os.clone(), eb.clone());
            bridge.start_forwarding();

            let sm_hook = state_machine.clone();
            let ss_hook = ss.clone();
            let eb_hook = eb.clone();
            tauri::async_runtime::spawn(async move {
                HookListener::start(sm_hook, ss_hook, eb_hook, None);
                info!("Hook listener started");
            });

            // Start directory tracker (OSC 7 parsing for terminal cwd)
            let ss_dir = ss.clone();
            let eb_dir = eb.clone();
            let os_dir = os.clone();
            tauri::async_runtime::spawn(async move {
                claude_tabs_ext_directory_tracker::start(eb_dir, ss_dir, os_dir);
            });
            info!("Directory tracker started");

            // PTY exit handler: close PTY and remove session
            {
                let mut exit_receiver = os.subscribe();
                let ss_exit = ss.clone();
                let eb_exit = eb.clone();
                let pm_exit = pty_manager.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        match exit_receiver.recv().await {
                            Ok(chunk) => {
                                if chunk.data.is_empty() {
                                    // PTY exited for this session
                                    let session_id = chunk.session_id.clone();
                                    if ss_exit.get(&session_id).await.is_some() {
                                        let _ = pm_exit.close(&session_id);
                                        ss_exit.remove(&session_id).await;

                                        let remaining = ss_exit.list().await;
                                        let new_active = remaining.first().map(|s| s.id.clone());
                                        ss_exit.set_active(new_active).await;

                                        let event = claude_tabs_core::Event::new(
                                            "session.closed",
                                            serde_json::json!({ "session_id": session_id }),
                                        );
                                        eb_exit.emit(event).await;
                                        info!(session_id = %session_id, "Session auto-closed on PTY exit");
                                    }
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                });
            }

            // Title sync background task: generate titles from JSONL first prompt
            {
                let ss_title = ss.clone();
                let eb_title = eb.clone();
                let scanner_title = Arc::new(SessionScanner::new().expect("HOME environment variable must be set"));
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(10)).await;

                        let sessions = ss_title.list().await;
                        for session in &sessions {
                            // Skip user-renamed sessions
                            let is_user_set = session.metadata.get("user_set_title")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            if is_user_set {
                                continue;
                            }

                            // Skip if already titled from JSONL
                            let has_jsonl_title = session.metadata.get("jsonl_title")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            if has_jsonl_title {
                                continue;
                            }

                            let claude_sid = match session.metadata.get("claude_session_id")
                                .and_then(|v| v.as_str())
                            {
                                Some(id) => id.to_string(),
                                None => continue,
                            };

                            // Extract first user prompt from JSONL
                            if let Some(prompt) = scanner_title.extract_first_prompt(&claude_sid) {
                                let new_title = if prompt.len() > 80 {
                                    let truncated: String = prompt.chars().take(77).collect();
                                    format!("{}...", truncated)
                                } else {
                                    prompt
                                };

                                if new_title != session.title {
                                    ss_title.rename(&session.id, &new_title).await;
                                    ss_title.set_metadata(
                                        &session.id,
                                        "jsonl_title",
                                        serde_json::Value::Bool(true),
                                    ).await;

                                    let event = claude_tabs_core::Event::new(
                                        "session.renamed",
                                        serde_json::json!({
                                            "session_id": session.id,
                                            "title": new_title,
                                            "source": "jsonl_prompt",
                                        }),
                                    );
                                    eb_title.emit(event).await;
                                    debug!(session_id = %session.id, title = %new_title, "Title set from JSONL first prompt");
                                }
                            }
                        }
                    }
                });
            }

            // Session recording: listen for hook.SessionStart to immediately persist to DB
            {
                let mut hook_receiver = eb.receiver();
                let ss_record = ss.clone();
                let storage_record = storage.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        match hook_receiver.recv().await {
                            Ok(event) => {
                                if event.topic != "hook.SessionStart" {
                                    continue;
                                }
                                let session_id = event.payload.get("session_id")
                                    .and_then(|v| v.as_str()).unwrap_or("").to_string();
                                let claude_sid = event.payload.get("claude_session_id")
                                    .and_then(|v| v.as_str()).unwrap_or("").to_string();
                                if claude_sid.is_empty() { continue; }

                                // Check if already recorded
                                if let Ok(Some(_)) = storage_record.get_session_metadata(&claude_sid).await {
                                    continue;
                                }

                                // Get session info from in-memory store
                                let (project_path, title) = if let Some(s) = ss_record.get(&session_id).await {
                                    (s.working_directory.clone().unwrap_or_default(), s.title.clone())
                                } else {
                                    (String::new(), String::new())
                                };

                                let meta = claude_tabs_storage::SessionMetadata {
                                    claude_session_id: claude_sid.clone(),
                                    project_path,
                                    custom_title: if title.is_empty() { None } else { Some(title) },
                                    user_set_title: false,
                                    generated_title: None,
                                    hidden: false,
                                    previous_session_id: None,
                                    last_known_state: Some("active".to_string()),
                                    last_state_change_at: None,
                                    created_at: String::new(),
                                    updated_at: String::new(),
                                };
                                let _ = storage_record.upsert_session_metadata(&meta).await;
                                info!(claude_session_id = %claude_sid, "Session recorded in DB");
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                });
            }

            // State reconciliation task: verify PTY liveness, persist state to DB
            {
                let ss_recon = ss.clone();
                let eb_recon = eb.clone();
                let sm_recon = state_machine.clone();
                let pm_recon = pty_manager.clone();
                let storage_recon = storage.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(15)).await;

                        let sessions = ss_recon.list().await;
                        for session in &sessions {
                            // Check PTY liveness
                            if !pm_recon.is_alive(&session.id)
                                && matches!(session.state, SessionState::Running | SessionState::YourTurn | SessionState::Paused | SessionState::Completed)
                            {
                                match sm_recon
                                    .transition_session(&session.id, SessionState::Idle, "reconciliation.pty_dead")
                                    .await
                                {
                                    Ok(transition) => {
                                        let event = claude_tabs_core::Event::new(
                                            "session.state_changed",
                                            serde_json::json!({
                                                "session_id": session.id,
                                                "from": transition.from.as_str(),
                                                "to": "idle",
                                            }),
                                        );
                                        eb_recon.emit(event).await;
                                        warn!(
                                            session_id = %session.id,
                                            from = %transition.from,
                                            "Reconciled dead PTY: forced to idle"
                                        );
                                    }
                                    Err(e) => {
                                        debug!(session_id = %session.id, error = %e, "Reconciliation transition skipped");
                                    }
                                }
                            }

                            // Persist state to DB for sessions with claude_session_id
                            if let Some(claude_sid) = session.metadata.get("claude_session_id")
                                .and_then(|v| v.as_str())
                            {
                                let existing = storage_recon.get_session_metadata(claude_sid).await.ok().flatten();
                                let meta = if let Some(mut m) = existing {
                                    m.last_known_state = Some(session.state.as_str().to_string());
                                    m
                                } else {
                                    claude_tabs_storage::SessionMetadata {
                                        claude_session_id: claude_sid.to_string(),
                                        project_path: session.working_directory.clone().unwrap_or_default(),
                                        custom_title: Some(session.title.clone()),
                                        user_set_title: false,
                                        generated_title: None,
                                        hidden: false,
                                        previous_session_id: None,
                                        last_known_state: Some(session.state.as_str().to_string()),
                                        last_state_change_at: None,
                                        created_at: String::new(),
                                        updated_at: String::new(),
                                    }
                                };
                                let _ = storage_recon.upsert_session_metadata(&meta).await;
                            }
                        }
                    }
                });
            }

            info!("IPC bridge started");
            Ok(())
        })
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // On macOS, hide the window instead of closing
                #[cfg(target_os = "macos")]
                {
                    _window.hide().unwrap();
                    api.prevent_close();
                }
                #[cfg(not(target_os = "macos"))]
                let _ = api;
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
