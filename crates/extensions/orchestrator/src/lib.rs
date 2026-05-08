//! Workflow orchestration extension.
//!
//! Drives a Claude Code session through pre-authored workflow stages
//! by judging stage completion via the `claude -p` CLI and PTY-typing
//! the next stage's prompt when the current stage is done.

pub mod guardrails;
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
        Ok(())
    }

    async fn deactivate(&mut self) -> Result<(), ExtensionError> {
        Ok(())
    }
}
