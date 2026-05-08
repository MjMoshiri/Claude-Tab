//! Workflow orchestration extension.
//!
//! Drives a Claude Code session through pre-authored workflow stages
//! by judging stage completion via the `claude -p` CLI and PTY-typing
//! the next stage's prompt when the current stage is done.

pub mod dispatcher;
pub mod guardrails;
pub mod transport;
pub mod injector;
pub mod judge;
pub mod runtime;
pub mod transcript;
pub mod workflow;

use async_trait::async_trait;
use claude_tabs_core::traits::extension::{
    ActivationContext, Extension, ExtensionError, ExtensionManifest,
};

pub struct OrchestratorExtension {
    manifest: ExtensionManifest,
}

impl OrchestratorExtension {
    pub fn new() -> Self {
        Self {
            manifest: ExtensionManifest::new("orchestrator", "Workflow Orchestrator")
                .with_description("Drives sessions through workflow stages"),
        }
    }
}

impl Default for OrchestratorExtension {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for OrchestratorExtension {
    fn manifest(&self) -> &ExtensionManifest {
        &self.manifest
    }

    async fn activate(&mut self, _ctx: &mut ActivationContext) -> Result<(), ExtensionError> {
        tracing::info!("OrchestratorExtension activated (no-op; runtime wired via create_orchestrator_future)");
        Ok(())
    }

    async fn deactivate(&mut self) -> Result<(), ExtensionError> {
        Ok(())
    }
}

use std::sync::Arc;

pub use dispatcher::Dispatcher;
pub use transport::auth::{default_config_dir, LocalAuth};

/// Construct the orchestrator runtime and return a future that runs the HTTP
/// listener until the process exits. Caller (src-tauri) is responsible for
/// supplying the shared `Arc<PtyManager>` and spawning the future onto an
/// async runtime.
///
/// On success, writes `~/.claude-tabs/orchestrator.token` (mode 0600) and
/// `~/.claude-tabs/orchestrator.port`. On failure to bind, the future
/// resolves to an `io::Error` and the caller should log it.
pub fn create_orchestrator_future(
    pty_manager: Arc<claude_tabs_pty::PtyManager>,
) -> std::io::Result<impl std::future::Future<Output = ()> + Send + 'static> {
    use crate::guardrails::AdvanceCounter;
    use crate::injector::Injector;
    use crate::judge::claude_cli::ClaudeCli;
    use crate::runtime::store::RunStore;
    use crate::workflow::loader::populate_registry;
    use crate::workflow::registry::WorkflowRegistry;

    let cfg_dir = default_config_dir();
    std::fs::create_dir_all(&cfg_dir)?;

    let db_path = cfg_dir.join("archive.db");
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    claude_tabs_storage::migrations::run_migrations(&conn)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    let store = Arc::new(RunStore::new(conn));

    let registry = Arc::new(WorkflowRegistry::new());
    let workflow_dir = cfg_dir.join("workflows");
    let _errs = populate_registry(&workflow_dir, &registry);

    let auth = Arc::new(LocalAuth::generate(cfg_dir)?);

    let injector = Arc::new(Injector::new(pty_manager));
    let dispatcher = Arc::new(Dispatcher {
        registry,
        store,
        judge: Arc::new(ClaudeCli),
        injector,
        counter: Arc::new(AdvanceCounter::new()),
    });

    let auth_for_serve = auth.clone();
    Ok(async move {
        if let Err(e) = transport::server::serve(dispatcher, auth_for_serve).await {
            tracing::error!("orchestrator http server crashed: {e}");
        }
    })
}
