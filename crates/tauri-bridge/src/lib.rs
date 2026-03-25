//! Tauri Bridge Crate
//!
//! Connects Tauri IPC to the core application:
//! - `commands`: Tauri command handlers for frontend IPC
//! - `ipc`: IpcBridge for event forwarding

pub mod commands;
pub mod ipc;

// Re-export public API
pub use ipc::{AppState, IpcBridge};

// Re-export command types for use in main.rs
pub use commands::{
    close_session, create_session, delete_history_session, delete_pack, delete_profile,
    delete_project_sessions, focus_window, fork_active_session, fork_session,
    get_active_session, get_claude_session, get_config_value, get_directory_preferences,
    get_profile, get_session_chain, get_session_content, is_app_active, launch_profile,
    list_claude_sessions, list_packs, list_profiles, list_sessions, remove_directory_preference,
    rename_session, request_attention, resize_pty, resume_session, save_pack, save_profile,
    save_config, set_active_session, set_config_value, set_directory_preference,
    set_session_hidden, set_session_state, setup_hooks, submit_input, trigger_title_generation, write_to_pty,
    CommandError, CreateSessionRequest, SessionInfo,
};
