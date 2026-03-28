use claude_tabs_core::config::Config;
use claude_tabs_core::event_bus::EventBus;
use claude_tabs_core::hook_listener::HookListener;
use claude_tabs_core::profile::{self, PackStore, Profile, ProfileInput, ProfileStore, WorkingDirConfig};
use claude_tabs_core::session::{Session, SessionStore};
use claude_tabs_core::skills::SkillManager;
use claude_tabs_core::traits::provider::PtySize;
use claude_tabs_pty::{OutputStream, PtyManager};
use rand::Rng;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use teloxide::dispatching::dialogue::{InMemStorage, GetChatId};
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

// --- Pairing ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PairingState {
    pub user_id: Option<u64>,
    pub chat_id: Option<i64>,
    pub username: Option<String>,
    pub paired_at: Option<String>,
}

fn pairing_file_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".claude-tabs")
        .join("telegram-pairing.json")
}

fn load_pairing() -> PairingState {
    let path = pairing_file_path();
    if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        PairingState::default()
    }
}

fn save_pairing(state: &PairingState) {
    let path = pairing_file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(&path, json);
    }
}

// --- Pending pairing code ---

#[derive(Debug, Clone)]
pub struct PendingCode {
    code: String,
    expires_at: std::time::Instant,
}

// --- Shared state for the bot ---

#[derive(Clone)]
pub struct BotDeps {
    pub config: Arc<Config>,
    pub profile_store: Arc<ProfileStore>,
    pub pack_store: Arc<PackStore>,
    pub session_store: Arc<SessionStore>,
    pub pty_manager: Arc<PtyManager>,
    pub output_stream: Arc<OutputStream>,
    pub event_bus: Arc<EventBus>,
    pub skill_manager: Arc<SkillManager>,
    pub pairing: Arc<RwLock<PairingState>>,
    pub pending_code: Arc<RwLock<Option<PendingCode>>>,
}

// --- Dialogue state machine ---

#[derive(Clone, Default, Debug)]
pub enum State {
    #[default]
    Idle,
    CollectingInputs {
        profile_id: String,
        profile_name: String,
        inputs: Vec<ProfileInput>,
        current_index: usize,
        collected: HashMap<String, String>,
        working_directory: Option<String>,
    },
}

type MyDialogue = Dialogue<State, InMemStorage<State>>;
type HandlerResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

// --- Public API ---

/// Generate a 6-character pairing code valid for 5 minutes.
/// Called from Tauri commands in the settings UI.
pub async fn generate_pairing_code(pending_code: Arc<RwLock<Option<PendingCode>>>) -> String {
    let code: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(6)
        .map(char::from)
        .collect::<String>()
        .to_uppercase();

    let mut lock = pending_code.write().await;
    *lock = Some(PendingCode {
        code: code.clone(),
        expires_at: std::time::Instant::now() + std::time::Duration::from_secs(300),
    });

    code
}

/// Get the current pairing status.
pub fn get_pairing_status() -> PairingState {
    load_pairing()
}

/// Disconnect the paired user.
pub async fn disconnect(pairing: Arc<RwLock<PairingState>>) {
    let empty = PairingState::default();
    save_pairing(&empty);
    *pairing.write().await = empty;
}

/// Create the bot dependencies and return the future that runs the bot loop.
/// The caller is responsible for spawning this onto an async runtime
/// (e.g. via `tauri::async_runtime::spawn`).
pub fn create_bot_future(
    config: Arc<Config>,
    profile_store: Arc<ProfileStore>,
    pack_store: Arc<PackStore>,
    session_store: Arc<SessionStore>,
    pty_manager: Arc<PtyManager>,
    output_stream: Arc<OutputStream>,
    event_bus: Arc<EventBus>,
    skill_manager: Arc<SkillManager>,
    pairing: Arc<RwLock<PairingState>>,
    pending_code: Arc<RwLock<Option<PendingCode>>>,
) -> impl std::future::Future<Output = ()> + Send {
    let deps = BotDeps {
        config,
        profile_store,
        pack_store,
        session_store,
        pty_manager,
        output_stream,
        event_bus,
        skill_manager,
        pairing,
        pending_code,
    };

    run_bot_loop(deps)
}

async fn run_bot_loop(deps: BotDeps) {
    loop {
        let token = deps.config.get_string("telegram.botToken").await;

        match token {
            Some(t) if !t.is_empty() => {
                info!("Telegram bot token found, starting bot...");
                if let Err(e) = run_bot(t, deps.clone()).await {
                    error!("Telegram bot exited with error: {}", e);
                }
                // If the bot exits (token revoked, network issue), wait and retry
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            }
            _ => {
                // No token configured, poll config every 5 seconds
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
}

async fn run_bot(
    token: String,
    deps: BotDeps,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bot = Bot::new(&token);

    let handler = Update::filter_message()
        .enter_dialogue::<Message, InMemStorage<State>, State>()
        .branch(dptree::case![State::Idle].endpoint(handle_message))
        .branch(
            dptree::case![State::CollectingInputs {
                profile_id,
                profile_name,
                inputs,
                current_index,
                collected,
                working_directory,
            }]
            .endpoint(handle_input_collection),
        );

    let handler = dptree::entry()
        .branch(handler)
        .branch(Update::filter_callback_query().endpoint(handle_callback));

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![
            InMemStorage::<State>::new(),
            deps.clone()
        ])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}

// --- Handlers ---

/// Handle any message in Idle state.
/// If it's a pairing code, try to pair. Otherwise, show profiles/packs.
async fn handle_message(
    bot: Bot,
    msg: Message,
    _dialogue: MyDialogue,
    deps: BotDeps,
) -> HandlerResult {
    let user_id = msg.from.as_ref().map(|u| u.id.0).unwrap_or(0);
    let chat_id = msg.chat.id;
    let text = msg.text().unwrap_or("").trim().to_string();

    // Check if this is a pairing attempt
    {
        let pending = deps.pending_code.read().await;
        if let Some(ref pc) = *pending {
            if pc.code.eq_ignore_ascii_case(&text) && pc.expires_at > std::time::Instant::now() {
                // Valid pairing code
                drop(pending);
                let mut pending_w = deps.pending_code.write().await;
                *pending_w = None; // Consume the code
                drop(pending_w);

                let username = msg
                    .from
                    .as_ref()
                    .and_then(|u| u.username.clone());

                let pairing_state = PairingState {
                    user_id: Some(user_id),
                    chat_id: Some(chat_id.0),
                    username,
                    paired_at: Some(chrono::Utc::now().to_rfc3339()),
                };
                save_pairing(&pairing_state);
                *deps.pairing.write().await = pairing_state;

                bot.send_message(chat_id, "Paired successfully! You can now launch sessions.")
                    .await?;
                return Ok(());
            }
        }
    }

    // Not a pairing code — check if user is paired
    let pairing = deps.pairing.read().await;
    if pairing.user_id != Some(user_id) {
        // Silently ignore unregistered users
        return Ok(());
    }
    drop(pairing);

    // Show profiles and packs
    show_main_menu(&bot, chat_id, &deps).await?;

    Ok(())
}

/// Show the main menu with profiles and packs as inline keyboard buttons.
async fn show_main_menu(
    bot: &Bot,
    chat_id: ChatId,
    deps: &BotDeps,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let profiles = deps.profile_store.list().await;
    let packs = deps.pack_store.list().await;

    if profiles.is_empty() && packs.is_empty() {
        bot.send_message(chat_id, "No profiles or packs configured.")
            .await?;
        return Ok(());
    }

    let mut keyboard: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    // Packs first
    for pack in &packs {
        keyboard.push(vec![InlineKeyboardButton::callback(
            format!("\u{1F4E6} {}", pack.name),
            format!("pack:{}", pack.id),
        )]);
    }

    // Then individual profiles
    for row in profiles.chunks(2) {
        let buttons: Vec<InlineKeyboardButton> = row
            .iter()
            .map(|p| {
                InlineKeyboardButton::callback(
                    format!("\u{1F464} {}", p.name),
                    format!("profile:{}", p.id),
                )
            })
            .collect();
        keyboard.push(buttons);
    }

    bot.send_message(chat_id, "Select a profile or pack:")
        .reply_markup(InlineKeyboardMarkup::new(keyboard))
        .await?;

    Ok(())
}

/// Handle callback queries from inline keyboard buttons.
async fn handle_callback(
    bot: Bot,
    q: CallbackQuery,
    deps: BotDeps,
    storage: std::sync::Arc<InMemStorage<State>>,
) -> HandlerResult {
    let user_id = q.from.id.0;

    // Check pairing
    let pairing = deps.pairing.read().await;
    if pairing.user_id != Some(user_id) {
        return Ok(());
    }
    drop(pairing);

    bot.answer_callback_query(&q.id).await?;

    let data = q.data.clone().unwrap_or_default();
    let chat_id = q.chat_id().unwrap_or(ChatId(0));

    if let Some(pack_id) = data.strip_prefix("pack:") {
        // Show profiles in the pack
        if let Some(pack) = deps.pack_store.get(pack_id).await {
            let mut keyboard: Vec<Vec<InlineKeyboardButton>> = Vec::new();

            for pid in &pack.profile_ids {
                if let Some(profile) = deps.profile_store.get(pid).await {
                    keyboard.push(vec![InlineKeyboardButton::callback(
                        format!("\u{1F464} {}", profile.name),
                        format!("profile:{}", profile.id),
                    )]);
                }
            }

            keyboard.push(vec![InlineKeyboardButton::callback(
                "\u{2B05}\u{FE0F} Back".to_string(),
                "back".to_string(),
            )]);

            bot.send_message(chat_id, &format!("Pack: {}", pack.name))
                .reply_markup(InlineKeyboardMarkup::new(keyboard))
                .await?;
        }
    } else if let Some(profile_id) = data.strip_prefix("profile:") {
        // Launch or collect inputs for this profile
        if let Some(profile) = deps.profile_store.get(profile_id).await {
            let required_inputs: Vec<ProfileInput> = profile
                .inputs
                .iter()
                .filter(|i| i.required)
                .cloned()
                .collect();

            // Check if we need a working directory from the user
            let needs_wd_prompt = matches!(profile.working_directory, Some(WorkingDirConfig::Prompt));

            if required_inputs.is_empty() && !needs_wd_prompt {
                // Can launch immediately
                bot.send_message(chat_id, &format!("Launching \"{}\"...", profile.name))
                    .await?;
                launch_and_capture(bot, chat_id, &profile, HashMap::new(), None, &deps).await?;
            } else {
                // Need to collect inputs
                let mut all_inputs = required_inputs;
                if needs_wd_prompt {
                    // Add a synthetic input for working directory
                    all_inputs.insert(
                        0,
                        ProfileInput {
                            key: "__working_directory__".to_string(),
                            label: "Working directory".to_string(),
                            placeholder: Some("/path/to/project".to_string()),
                            input_type: "text".to_string(),
                            required: true,
                            options: None,
                            default: None,
                        },
                    );
                }

                // Ask for the first input
                let first = &all_inputs[0];
                send_input_prompt(&bot, chat_id, first).await?;

                use teloxide::dispatching::dialogue::Storage;
                storage
                    .update_dialogue(chat_id, State::CollectingInputs {
                        profile_id: profile.id.clone(),
                        profile_name: profile.name.clone(),
                        inputs: all_inputs,
                        current_index: 0,
                        collected: HashMap::new(),
                        working_directory: match &profile.working_directory {
                            Some(WorkingDirConfig::Fixed { path }) => Some(path.clone()),
                            _ => None,
                        },
                    })
                    .await
                    .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            }
        }
    } else if data == "back" {
        show_main_menu(&bot, chat_id, &deps).await?;
    }

    Ok(())
}

/// Send a prompt for a profile input, using inline keyboard for select-type inputs.
async fn send_input_prompt(
    bot: &Bot,
    chat_id: ChatId,
    input: &ProfileInput,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let prompt = if let Some(ref ph) = input.placeholder {
        format!("{} (e.g. {}):", input.label, ph)
    } else {
        format!("{}:", input.label)
    };

    if input.input_type == "select" {
        if let Some(ref options) = input.options {
            let keyboard: Vec<Vec<InlineKeyboardButton>> = options
                .chunks(2)
                .map(|chunk| {
                    chunk
                        .iter()
                        .map(|opt| {
                            InlineKeyboardButton::callback(
                                opt.clone(),
                                format!("input:{}", opt),
                            )
                        })
                        .collect()
                })
                .collect();

            bot.send_message(chat_id, &prompt)
                .reply_markup(InlineKeyboardMarkup::new(keyboard))
                .await?;
            return Ok(());
        }
    }

    bot.send_message(chat_id, &prompt).await?;
    Ok(())
}

/// Handle text messages while collecting inputs.
async fn handle_input_collection(
    bot: Bot,
    msg: Message,
    dialogue: MyDialogue,
    deps: BotDeps,
    (profile_id, profile_name, inputs, current_index, collected, working_directory): (
        String,
        String,
        Vec<ProfileInput>,
        usize,
        HashMap<String, String>,
        Option<String>,
    ),
) -> HandlerResult {
    let user_id = msg.from.as_ref().map(|u| u.id.0).unwrap_or(0);

    // Verify pairing
    let pairing = deps.pairing.read().await;
    if pairing.user_id != Some(user_id) {
        return Ok(());
    }
    drop(pairing);

    let text = msg.text().unwrap_or("").trim().to_string();
    if text.is_empty() {
        bot.send_message(msg.chat.id, "Please send a text value.")
            .await?;
        return Ok(());
    }

    let mut collected = collected;
    let current_input = &inputs[current_index];
    let mut working_directory = working_directory;

    if current_input.key == "__working_directory__" {
        working_directory = Some(text);
    } else {
        collected.insert(current_input.key.clone(), text);
    }

    let next_index = current_index + 1;

    if next_index >= inputs.len() {
        // All inputs collected — launch
        dialogue.update(State::Idle).await?;

        bot.send_message(msg.chat.id, &format!("Launching \"{}\"...", profile_name))
            .await?;

        if let Some(profile) = deps.profile_store.get(&profile_id).await {
            // Resolve working directory from inputs if needed
            let wd = match &profile.working_directory {
                Some(WorkingDirConfig::FromInput { key }) => collected.get(key).cloned().or(working_directory),
                _ => working_directory,
            };
            launch_and_capture(bot, msg.chat.id, &profile, collected, wd, &deps).await?;
        }
    } else {
        // Ask for the next input
        let next_input = &inputs[next_index];
        send_input_prompt(&bot, msg.chat.id, next_input).await?;

        dialogue
            .update(State::CollectingInputs {
                profile_id,
                profile_name,
                inputs,
                current_index: next_index,
                collected,
                working_directory,
            })
            .await?;
    }

    Ok(())
}

// --- Session launch + /rc capture ---

fn get_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    std::process::Command::new(&shell)
        .args(["-l", "-c", "echo $PATH"])
        .output()
        .ok()
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(s)
            }
        })
}

fn policy_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".claude")
        .join("auto-accept-policies")
}

/// Launch a profile session and capture the /rc URL.
async fn launch_and_capture(
    bot: Bot,
    chat_id: ChatId,
    profile: &Profile,
    input_values: HashMap<String, String>,
    working_directory_override: Option<String>,
    deps: &BotDeps,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Resolve working directory
    let working_directory = match &profile.working_directory {
        Some(WorkingDirConfig::Fixed { path }) => Some(path.clone()),
        Some(WorkingDirConfig::FromInput { key }) => input_values.get(key).cloned(),
        _ => working_directory_override,
    };

    // Resolve prompt template
    let initial_prompt = profile.prompt_template.as_ref().map(|template| {
        deps.profile_store.resolve_prompt(template, &input_values)
    });

    // Handle system prompt
    let system_prompt = if profile.system_prompt.is_some() {
        profile.system_prompt.clone()
    } else if let Some(ref file_name) = profile.system_prompt_file {
        profile::read_system_prompt_content(file_name).ok()
    } else {
        None
    };

    // Sync skills
    if let Some(ref skills) = profile.skills {
        if !skills.is_empty() {
            if let Err(e) = deps.skill_manager.sync_skills(skills) {
                warn!("Failed to sync skills: {}", e);
            }
        }
    }

    // Create session
    let mut session = Session::new("claude-code");
    session.title = profile.name.clone();
    if let Some(ref dir) = working_directory {
        session.working_directory = Some(dir.clone());
    }
    let session_id = session.id.clone();

    // Build env vars (mirrors create_session in commands.rs)
    let mut env = HashMap::new();
    env.insert("TERM".to_string(), "xterm-256color".to_string());
    env.insert("CLAUDE_TABS_SESSION_ID".to_string(), session_id.clone());

    if let Some(shell_path) = get_shell_path() {
        env.insert("PATH".to_string(), shell_path);
    }

    env.insert(
        "CLAUDE_TABS_SOCKET".to_string(),
        HookListener::socket_path().to_string_lossy().to_string(),
    );

    // Auto-accept policy
    if let Some(serde_json::Value::Bool(true)) = deps.config.get("autoAccept.enabled").await {
        let default_policy = deps
            .config
            .get("autoAccept.defaultPolicy")
            .await
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_default();
        if !default_policy.is_empty() {
            env.insert("AUTO_ACCEPT_POLICY".to_string(), default_policy.clone());
        }
        let dir = policy_dir();
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(&session_id), &default_policy);

        if let Some(serde_json::Value::String(model)) = deps.config.get("autoAccept.model").await {
            env.insert("AUTO_ACCEPT_MODEL".to_string(), model);
        }
        if let Some(serde_json::Value::String(mode)) = deps.config.get("autoAccept.mode").await {
            env.insert("AUTO_ACCEPT_MODE".to_string(), mode);
        }
    }

    // Handle system prompt
    if let Some(ref sp) = system_prompt {
        // Will be passed as --append-system-prompt arg
        let _ = sp;
    }

    // Build args
    let mut args: Vec<String> = Vec::new();
    if let Some(ref tools) = profile.allowed_tools {
        if !tools.is_empty() {
            args.push("--allowedTools".to_string());
            args.push(tools.join(","));
        }
    }
    if let Some(ref model) = profile.model {
        args.push("--model".to_string());
        args.push(model.clone());
    }
    if let Some(ref sp) = system_prompt {
        args.push("--append-system-prompt".to_string());
        args.push(sp.clone());
    }
    if profile.dangerously_skip_permissions {
        args.push("--dangerously-skip-permissions".to_string());
    }
    // Add prompt as positional arg
    if let Some(ref prompt) = initial_prompt {
        args.push(prompt.clone());
    }

    let size = PtySize { rows: 24, cols: 80 };

    // Spawn PTY
    let reader = match deps.pty_manager.spawn(
        &session_id,
        "claude",
        &args,
        working_directory.as_deref(),
        &env,
        size,
    ) {
        Ok(r) => r,
        Err(e) => {
            bot.send_message(chat_id, &format!("Failed to launch session: {}", e))
                .await?;
            return Ok(());
        }
    };

    deps.output_stream.start_reading(session_id.clone(), reader);
    deps.session_store.add(session.clone()).await;
    deps.session_store
        .set_active(Some(session_id.clone()))
        .await;

    // Store profile metadata
    deps.session_store
        .set_metadata(
            &session_id,
            "profile_id",
            serde_json::Value::String(profile.id.clone()),
        )
        .await;
    deps.session_store
        .set_metadata(
            &session_id,
            "telegram_launched",
            serde_json::Value::Bool(true),
        )
        .await;

    // Emit session.created event
    let event = claude_tabs_core::Event::new(
        "session.created",
        serde_json::json!({
            "session_id": session_id,
            "provider_id": "claude-code",
        }),
    );
    deps.event_bus.emit(event).await;

    // Apply profile auto-accept policy
    if let Some(ref policy) = profile.auto_accept_policy {
        if !policy.is_empty() {
            let dir = policy_dir();
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join(&session_id), policy);
        }
    }

    info!(session_id = %session_id, profile = %profile.name, "Telegram-launched session created");

    // Now wait for SessionStart hook, then write /rc and capture URL
    spawn_rc_capture(bot, chat_id, session_id, profile.name.clone(), deps.clone());

    Ok(())
}

/// Spawn a background task that waits for the session to start,
/// writes /remote-control, and captures the URL.
fn spawn_rc_capture(
    bot: Bot,
    chat_id: ChatId,
    session_id: String,
    profile_name: String,
    deps: BotDeps,
) {
    tokio::spawn(async move {
        // Wait for the hook.SessionStart event for this session
        let mut receiver = deps.event_bus.receiver();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);

        let mut session_started = false;

        loop {
            let timeout = tokio::time::timeout_at(deadline, receiver.recv()).await;

            match timeout {
                Ok(Ok(event)) => {
                    if event.topic == "hook.SessionStart" {
                        let ev_session_id = event
                            .payload
                            .get("session_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if ev_session_id == session_id {
                            session_started = true;
                            break;
                        }
                    }
                }
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(_)) => break,
                Err(_) => break, // Timeout
            }
        }

        if !session_started {
            let _ = bot
                .send_message(chat_id, "Session timed out waiting to start.")
                .await;
            return;
        }

        // Small delay for Claude Code to fully initialize
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Write /remote-control to the PTY
        if let Err(e) = deps
            .pty_manager
            .write_data(&session_id, b"/remote-control\r")
        {
            error!("Failed to write /rc to PTY: {}", e);
            let _ = bot
                .send_message(chat_id, "Failed to activate remote control.")
                .await;
            return;
        }

        debug!(session_id = %session_id, "Wrote /remote-control to PTY");

        // Subscribe to PTY output and watch for the URL
        let mut output_receiver = deps.output_stream.subscribe();
        let url_deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        let url_pattern =
            Regex::new(r"https://claude\.ai/code/session_[A-Za-z0-9]+").expect("valid regex");

        let mut accumulated = String::new();

        loop {
            let timeout = tokio::time::timeout_at(url_deadline, output_receiver.recv()).await;

            match timeout {
                Ok(Ok(chunk)) => {
                    if chunk.session_id.as_ref() != session_id {
                        continue;
                    }
                    if chunk.data.is_empty() {
                        break; // PTY closed
                    }

                    // Accumulate output and search for URL
                    if let Ok(text) = std::str::from_utf8(&chunk.data) {
                        accumulated.push_str(text);
                    }

                    if let Some(mat) = url_pattern.find(&accumulated) {
                        let url = mat.as_str().to_string();
                        info!(session_id = %session_id, url = %url, "Captured /rc URL");

                        let keyboard = InlineKeyboardMarkup::new(vec![vec![
                            InlineKeyboardButton::url(
                                "Open in Claude \u{2197}\u{FE0F}".to_string(),
                                url.parse().expect("valid url"),
                            ),
                        ]]);

                        let _ = bot
                            .send_message(
                                chat_id,
                                &format!(
                                    "\u{2705} Session \"{}\" is ready!",
                                    profile_name
                                ),
                            )
                            .reply_markup(keyboard)
                            .await;
                        return;
                    }
                }
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
                Ok(Err(_)) => break,
                Err(_) => break, // Timeout
            }
        }

        let _ = bot
            .send_message(chat_id, "Failed to capture remote control URL. The session is running but /rc may not have activated.")
            .await;
    });
}
