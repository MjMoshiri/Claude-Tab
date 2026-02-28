pub mod config;
pub mod event_bus;
pub mod events;
pub mod hook_listener;
pub mod plugin_host;
pub mod profile;
pub mod session;
pub mod skills;
pub mod state_machine;
pub mod title;
pub mod traits;
pub mod worktree;

// Core types
pub use config::{Config, ConfigLayer};
pub use event_bus::{Event, EventBus, EventHandler, Subscription};
pub use hook_listener::HookListener;
pub use plugin_host::PluginHost;
pub use session::{Session, SessionId, SessionStore};
pub use state_machine::{SessionState, StateMachine, Transition, TransitionError};

// Original traits
pub use traits::channel::NotificationChannel;
pub use traits::detector::{DetectionResult, DetectorInput, StateDetector};
pub use traits::extension::{ActivationContext, Extension, ExtensionError, ExtensionManifest};
pub use traits::provider::{ProviderRegistry, PtyHandle, SessionConfig, SessionProvider};
pub use traits::reaction::{Reaction, ReactionTrigger};

// New traits for modular architecture
pub use traits::archiver::{ArchivedSessionData, ArchiverError, SearchResultData, SessionArchiver};
pub use traits::buffer::{BufferError, SessionBufferProvider};
pub use traits::factory::{CreateSessionConfig, CreateSessionResult, FactoryError, SessionFactory};
pub use traits::output::{OutputChunk, OutputError, OutputReader, SessionOutputStream, SessionOutputSubscriber};

// Profile types
pub use profile::{
    cleanup_temp_mcp_config, list_mcp_servers, list_system_prompts, read_system_prompt_content,
    write_filtered_mcp_config, McpServerEntry, Profile, ProfileInput, ProfileLaunchRequest,
    ProfileStore, SystemPromptEntry,
};

// Title resolution
pub use title::{generate_title_prompt, parse_title_response, resolve_title};

// Skills
pub use skills::{SkillInfo, SkillManager, SkillError};

// Worktree
pub use worktree::{WorktreeInfo, WorktreeError};

// Event topics
pub use events::topics;
