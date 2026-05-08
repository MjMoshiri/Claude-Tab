use crate::workflow::ir::{NextRef, Workflow};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub run_id: String,
    pub session_id: String,
    pub workflow_id: String,
    pub inputs: BTreeMap<String, serde_json::Value>,
    pub current_stage_id: String,
    pub status: RunStatus,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Paused,
    Done,
    Escalated,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    StayInStage,
    Complete,
    Escalate { reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum AdvanceOutcome {
    NoOp,
    Done,
    Inject { stage_id: String },
    Escalate { reason: String },
}

pub fn advance(run: &WorkflowRun, workflow: &Workflow, verdict: Verdict) -> AdvanceOutcome {
    match verdict {
        Verdict::StayInStage => AdvanceOutcome::NoOp,
        Verdict::Escalate { reason } => AdvanceOutcome::Escalate { reason },
        Verdict::Complete => {
            let stage = workflow
                .stages
                .get(&run.current_stage_id)
                .expect("run.current_stage_id must exist in workflow");
            match &stage.next {
                NextRef::Done => AdvanceOutcome::Done,
                NextRef::StageId(id) => AdvanceOutcome::Inject { stage_id: id.clone() },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::ir::*;

    fn run_at(stage: &str) -> WorkflowRun {
        WorkflowRun {
            run_id: "r".into(),
            session_id: "s".into(),
            workflow_id: "w".into(),
            inputs: BTreeMap::new(),
            current_stage_id: stage.into(),
            status: RunStatus::Running,
            started_at: 0,
            ended_at: None,
        }
    }

    fn wf_two_stage() -> Workflow {
        let mut stages = BTreeMap::new();
        stages.insert(
            "a".into(),
            Stage {
                id: "a".into(),
                goal: String::new(),
                prompt: "p".into(),
                completion: Completion::Criteria { criteria: "ok".into() },
                next: NextRef::StageId("b".into()),
                notes: Notes::Static,
            },
        );
        stages.insert(
            "b".into(),
            Stage {
                id: "b".into(),
                goal: String::new(),
                prompt: "p".into(),
                completion: Completion::Criteria { criteria: "ok".into() },
                next: NextRef::Done,
                notes: Notes::Static,
            },
        );
        Workflow {
            id: "w".into(),
            name: "w".into(),
            description: String::new(),
            model: "haiku".into(),
            inputs: vec![],
            stages,
            stage_order: vec!["a".into(), "b".into()],
        }
    }

    #[test]
    fn stay_yields_noop() {
        let run = run_at("a");
        let wf = wf_two_stage();
        assert_eq!(advance(&run, &wf, Verdict::StayInStage), AdvanceOutcome::NoOp);
    }

    #[test]
    fn complete_to_next_stage() {
        let run = run_at("a");
        let wf = wf_two_stage();
        assert_eq!(
            advance(&run, &wf, Verdict::Complete),
            AdvanceOutcome::Inject { stage_id: "b".into() }
        );
    }

    #[test]
    fn complete_to_done() {
        let run = run_at("b");
        let wf = wf_two_stage();
        assert_eq!(advance(&run, &wf, Verdict::Complete), AdvanceOutcome::Done);
    }

    #[test]
    fn escalate_propagates_reason() {
        let run = run_at("a");
        let wf = wf_two_stage();
        assert_eq!(
            advance(&run, &wf, Verdict::Escalate { reason: "stuck".into() }),
            AdvanceOutcome::Escalate { reason: "stuck".into() }
        );
    }
}
