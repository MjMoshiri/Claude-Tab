use crate::guardrails::{AdvanceCounter, GuardrailError};
use crate::injector::Injector;
use crate::judge::{completion::judge_completion, notes::draft_notes, JudgeRunner};
use crate::runtime::render::render;
use crate::runtime::run::{advance, AdvanceOutcome, RunStatus, Verdict};
use crate::runtime::store::{RunStore, StoreError};
use crate::transcript::Tail;
use crate::workflow::ir::{Completion, Notes, SessionEventKind, Workflow};
use crate::workflow::registry::WorkflowRegistry;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex as AsyncMutex;
use tracing::info;

#[derive(Debug, Error)]
pub enum DispatchError {
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("judge: {0}")]
    Judge(#[from] crate::judge::JudgeError),
    #[error("render: {0}")]
    Render(#[from] crate::runtime::render::RenderError),
    #[error("inject: {0}")]
    Inject(#[from] crate::injector::InjectError),
    #[error("guardrail: {0}")]
    Guardrail(#[from] GuardrailError),
    #[error("no workflow attached for session {0}")]
    NoRun(String),
    #[error("workflow `{0}` not registered")]
    UnknownWorkflow(String),
    #[error("stage `{0}` not found in workflow `{1}` (registry changed mid-run?)")]
    UnknownStage(String, String),
}

pub struct Dispatcher {
    pub registry: Arc<WorkflowRegistry>,
    pub store: Arc<RunStore>,
    pub judge: Arc<dyn JudgeRunner>,
    pub injector: Arc<Injector>,
    pub counter: Arc<AdvanceCounter>,
    /// Per-session async lock. Serializes `on_turn_end` / `on_session_start`
    /// / `do_inject` for a given session so concurrent HTTP requests can't
    /// double-advance, double-inject, or interleave DB+PTY writes.
    session_locks: AsyncMutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl Dispatcher {
    pub fn new(
        registry: Arc<WorkflowRegistry>,
        store: Arc<RunStore>,
        judge: Arc<dyn JudgeRunner>,
        injector: Arc<Injector>,
        counter: Arc<AdvanceCounter>,
    ) -> Self {
        Self {
            registry,
            store,
            judge,
            injector,
            counter,
            session_locks: AsyncMutex::new(HashMap::new()),
        }
    }

    async fn session_lock(&self, session_id: &str) -> Arc<AsyncMutex<()>> {
        let mut map = self.session_locks.lock().await;
        map.entry(session_id.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }
}

impl Dispatcher {
    pub async fn on_turn_end(
        &self,
        session_id: &str,
        transcript_path: &Path,
    ) -> Result<(), DispatchError> {
        let lock = self.session_lock(session_id).await;
        let _guard = lock.lock().await;

        let run = self
            .store
            .load_by_session(session_id)?
            .ok_or_else(|| DispatchError::NoRun(session_id.into()))?;
        if run.status != RunStatus::Running {
            return Ok(());
        }
        let workflow = self
            .registry
            .get(&run.workflow_id)
            .ok_or_else(|| DispatchError::UnknownWorkflow(run.workflow_id.clone()))?;
        let stage = workflow
            .stages
            .get(&run.current_stage_id)
            .ok_or_else(|| {
                DispatchError::UnknownStage(
                    run.current_stage_id.clone(),
                    run.workflow_id.clone(),
                )
            })?
            .clone();

        let tail = Tail::read(transcript_path, 80).unwrap_or(Tail { entries: vec![] });

        let verdict = match &stage.completion {
            Completion::Criteria { .. } => {
                judge_completion(&*self.judge, &workflow.model, &stage, &run, &tail).await?
            }
            Completion::ToolUseMatch { matcher } => {
                let matched = tail
                    .tool_uses()
                    .any(|(name, input)| matches_tool_use(matcher, name, input));
                if matched {
                    Verdict::Complete
                } else {
                    Verdict::StayInStage
                }
            }
            Completion::SessionEvent { .. } => Verdict::StayInStage, // advanced via /session-start
        };

        match advance(&run, &workflow, verdict) {
            AdvanceOutcome::NoOp => Ok(()),
            AdvanceOutcome::Done => {
                self.store.set_status(&run.run_id, RunStatus::Done)?;
                Ok(())
            }
            AdvanceOutcome::Inject { stage_id } => {
                self.do_inject(&run, &workflow, &stage_id, &tail).await
            }
            AdvanceOutcome::Escalate { .. } => {
                self.store.set_status(&run.run_id, RunStatus::Escalated)?;
                Ok(())
            }
        }
    }

    pub async fn on_session_start(
        &self,
        session_id: &str,
        source: &str,
    ) -> Result<(), DispatchError> {
        let lock = self.session_lock(session_id).await;
        let _guard = lock.lock().await;

        let Some(run) = self.store.load_by_session(session_id)? else {
            return Ok(());
        };
        if run.status != RunStatus::Running {
            return Ok(());
        }
        if source != "compact" {
            return Ok(());
        }
        let workflow = self
            .registry
            .get(&run.workflow_id)
            .ok_or_else(|| DispatchError::UnknownWorkflow(run.workflow_id.clone()))?;
        let stage = workflow
            .stages
            .get(&run.current_stage_id)
            .ok_or_else(|| {
                DispatchError::UnknownStage(
                    run.current_stage_id.clone(),
                    run.workflow_id.clone(),
                )
            })?
            .clone();

        if let Completion::SessionEvent { on } = &stage.completion {
            if matches!(on, SessionEventKind::Compact) {
                match advance(&run, &workflow, Verdict::Complete) {
                    AdvanceOutcome::Inject { stage_id } => {
                        let empty = Tail { entries: vec![] };
                        self.do_inject(&run, &workflow, &stage_id, &empty).await?;
                    }
                    AdvanceOutcome::Done => {
                        self.store.set_status(&run.run_id, RunStatus::Done)?;
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    pub async fn do_inject(
        &self,
        run: &crate::runtime::run::WorkflowRun,
        workflow: &Workflow,
        next_stage_id: &str,
        tail: &Tail,
    ) -> Result<(), DispatchError> {
        self.counter.record_advance(&run.run_id)?;
        let next_stage = workflow
            .stages
            .get(next_stage_id)
            .ok_or_else(|| {
                DispatchError::UnknownStage(next_stage_id.into(), run.workflow_id.clone())
            })?
            .clone();

        // Pre-check: bail before any DB advance if the PTY is already gone.
        // Shrinks the window where we'd commit an advance the user can never
        // see typed (the txn-hole reviewer flagged dispatcher.rs:154-161).
        if !self.injector.is_alive(&run.session_id) {
            return Err(DispatchError::Inject(
                crate::injector::InjectError::NotFound(run.session_id.clone()),
            ));
        }

        let notes = if let Notes::Dynamic { hint } = &next_stage.notes {
            Some(draft_notes(&*self.judge, &workflow.model, hint, run, tail).await?)
        } else {
            None
        };

        let rendered = render(&next_stage, &run.inputs, notes.as_deref())?;
        self.store.advance_run(&run.run_id, next_stage_id)?;
        self.store
            .record_history(&run.run_id, next_stage_id, "complete", None)?;
        info!(
            session_id = %run.session_id,
            next_stage = %next_stage_id,
            "injecting next stage"
        );
        // If the PTY write fails after we've advanced the DB, mark the run as
        // Error so the UI surfaces the inconsistency rather than silently
        // leaving the user on a stage whose prompt was never typed.
        if let Err(e) = self.injector.write_user_prompt(&run.session_id, &rendered) {
            let _ = self.store.set_status(&run.run_id, RunStatus::Error);
            return Err(e.into());
        }
        Ok(())
    }
}

fn matches_tool_use(
    matcher: &crate::workflow::ir::ToolUseMatcher,
    name: &str,
    input: &serde_json::Value,
) -> bool {
    if matcher.tool != name {
        return false;
    }
    for (field, pattern) in &matcher.input {
        let val = input.get(field).and_then(|v| v.as_str()).unwrap_or("");
        let re = match regex::Regex::new(pattern) {
            Ok(r) => r,
            Err(_) => return false,
        };
        if !re.is_match(val) {
            return false;
        }
    }
    true
}
