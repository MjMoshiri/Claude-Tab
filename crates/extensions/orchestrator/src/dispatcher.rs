use crate::guardrails::{AdvanceCounter, GuardrailError};
use crate::injector::Injector;
use crate::judge::{completion::judge_completion, notes::draft_notes, JudgeRunner};
use crate::runtime::render::render;
use crate::runtime::run::{advance, AdvanceOutcome, RunStatus, Verdict};
use crate::runtime::store::{RunStore, StoreError};
use crate::transcript::Tail;
use crate::workflow::ir::{Completion, Notes, SessionEventKind, Workflow};
use crate::workflow::registry::WorkflowRegistry;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;
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
}

pub struct Dispatcher {
    pub registry: Arc<WorkflowRegistry>,
    pub store: Arc<RunStore>,
    pub judge: Arc<dyn JudgeRunner>,
    pub injector: Arc<Injector>,
    pub counter: Arc<AdvanceCounter>,
}

impl Dispatcher {
    pub async fn on_turn_end(
        &self,
        session_id: &str,
        transcript_path: &Path,
    ) -> Result<(), DispatchError> {
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
            .expect("stage exists; compiler validated")
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
        let Some(run) = self.store.load_by_session(session_id)? else {
            return Ok(());
        };
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
            .expect("validated")
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
            .expect("validated")
            .clone();

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
        self.injector
            .write_user_prompt(&run.session_id, &rendered)?;
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
