use super::{JudgeError, JudgeRunner};
use crate::guardrails::MAX_JUDGE_PROMPT_BYTES;
use crate::runtime::run::WorkflowRun;
use crate::transcript::Tail;

pub async fn draft_notes(
    runner: &dyn JudgeRunner,
    model: &str,
    hint: &str,
    _run: &WorkflowRun,
    tail: &Tail,
) -> Result<String, JudgeError> {
    let transcript_repr = tail.compact_repr(MAX_JUDGE_PROMPT_BYTES / 2);
    let prompt = format!(
        "You are drafting a short contextual note that will be appended \
         to the next stage's prompt. The author of the workflow gave you this hint:\n\n\
         <hint>{}</hint>\n\n\
         Recent transcript:\n{}\n\n\
         Output ONE concise sentence (≤30 words). No preamble, no quotes.",
        hint, transcript_repr
    );
    let raw = runner.run(&prompt, model).await?;
    Ok(raw.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::collections::BTreeMap;
    use crate::runtime::run::RunStatus;

    struct MockRunner {
        reply: String,
    }

    #[async_trait]
    impl JudgeRunner for MockRunner {
        async fn run(&self, _prompt: &str, _model: &str) -> Result<String, JudgeError> {
            Ok(self.reply.clone())
        }
    }

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

    #[tokio::test]
    async fn returns_trimmed_note() {
        let runner = MockRunner {
            reply: "  Mention docs are stale.  \n".into(),
        };
        let n = draft_notes(
            &runner,
            "haiku",
            "remind about docs",
            &run_at("a"),
            &Tail { entries: vec![] },
        )
        .await
        .unwrap();
        assert_eq!(n, "Mention docs are stale.");
    }
}
