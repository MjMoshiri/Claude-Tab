//! Session Factory Trait
//!
//! Defines the contract for orchestrating session creation.
//! This trait abstracts the session creation logic from the command handlers,
//! enabling different session creation strategies.

use crate::session::Session;
use crate::Event;
use async_trait::async_trait;
use std::collections::HashMap;

/// Configuration for creating a new session.
#[derive(Debug, Clone)]
pub struct CreateSessionConfig {
    /// Provider identifier (e.g., "claude-code", "bash")
    pub provider_id: String,

    /// Working directory for the session
    pub working_directory: Option<String>,

    /// Session title
    pub title: Option<String>,

    /// Claude session ID to resume from
    pub resume_claude_session_id: Option<String>,

    /// Whether to fork the session
    pub fork: bool,

    /// Initial prompt to send
    pub initial_prompt: Option<String>,

    /// Allowed tools for Claude
    pub allowed_tools: Option<Vec<String>>,

    /// Model to use
    pub model: Option<String>,

    /// System prompt to append
    pub system_prompt: Option<String>,

    /// Additional environment variables
    pub env: HashMap<String, String>,

    /// Initial PTY size
    pub initial_rows: u16,
    pub initial_cols: u16,
}

impl Default for CreateSessionConfig {
    fn default() -> Self {
        Self {
            provider_id: "bash".to_string(),
            working_directory: None,
            title: None,
            resume_claude_session_id: None,
            fork: false,
            initial_prompt: None,
            allowed_tools: None,
            model: None,
            system_prompt: None,
            env: HashMap::new(),
            initial_rows: 24,
            initial_cols: 80,
        }
    }
}

/// Result of successful session creation.
#[derive(Debug)]
pub struct CreateSessionResult {
    /// The created session
    pub session: Session,

    /// Events to emit after creation
    pub events: Vec<Event>,
}

/// Error types for session factory operations.
#[derive(Debug, thiserror::Error)]
pub enum FactoryError {
    #[error("Provider not found: {0}")]
    ProviderNotFound(String),

    #[error("PTY spawn failed: {0}")]
    PtySpawnFailed(String),

    #[error("Session store error: {0}")]
    SessionStoreError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Trait for orchestrating session creation.
///
/// The SessionFactory is responsible for:
/// - Creating the session object
/// - Spawning the PTY process
/// - Setting up environment variables
/// - Registering the session with the store
/// - Emitting creation events
///
/// # Example Implementation
/// ```ignore
/// struct DefaultSessionFactory {
///     pty_manager: Arc<PtyManager>,
///     session_store: Arc<SessionStore>,
///     event_bus: Arc<EventBus>,
/// }
///
/// #[async_trait]
/// impl SessionFactory for DefaultSessionFactory {
///     async fn create(&self, config: CreateSessionConfig) -> Result<CreateSessionResult, FactoryError> {
///         // Create session
///         let mut session = Session::new(&config.provider_id);
///
///         // Spawn PTY
///         let reader = self.pty_manager.spawn(...)?;
///
///         // Add to store
///         self.session_store.add(session.clone()).await;
///
///         // Return result with events to emit
///         Ok(CreateSessionResult {
///             session,
///             events: vec![Event::new("session.created", ...)],
///         })
///     }
/// }
/// ```
#[async_trait]
pub trait SessionFactory: Send + Sync + 'static {
    /// Create a new session with the given configuration.
    async fn create(&self, config: CreateSessionConfig) -> Result<CreateSessionResult, FactoryError>;

    /// Close an existing session.
    async fn close(&self, session_id: &str) -> Result<Vec<Event>, FactoryError>;
}
