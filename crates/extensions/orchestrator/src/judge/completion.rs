use super::{JudgeError, JudgeRunner};
use crate::guardrails::MAX_JUDGE_PROMPT_BYTES;
use crate::runtime::run::{Verdict, WorkflowRun};
use crate::transcript::Tail;
use crate::workflow::ir::{Completion, Stage};
use regex::Regex;
use serde::Deserialize;

#[derive(Deserialize)]
struct JudgeAnswer {
    complete: bool,
    #[serde(default)]
    #[allow(dead_code)]
    reason: String,
}

pub async fn judge_completion(
    runner: &dyn JudgeRunner,
    model: &str,
    stage: &Stage,
    _run: &WorkflowRun,
    tail: &Tail,
) -> Result<Verdict, JudgeError> {
    let criteria = match &stage.completion {
        Completion::Criteria { criteria } => criteria.clone(),
        _ => return Err(JudgeError::Malformed("non-criteria completion routed to LLM judge".into())),
    };

    let last_msg = tail.last_assistant_text().unwrap_or_default();
    let transcript_repr = tail.compact_repr(MAX_JUDGE_PROMPT_BYTES / 2);

    let prompt = format!(
        "You are a workflow stage completion judge.\n\n\
         Stage goal: {}\n\
         Completion criteria: {}\n\n\
         Recent transcript:\n{}\n\n\
         Last assistant message:\n{}\n\n\
         Has this stage met its completion criteria?\n\n\
         Output strictly:\n\
         <answer>{{\"complete\": true|false, \"reason\": \"<≤30 words>\"}}</answer>",
        stage.goal, criteria, transcript_repr, last_msg
    );

    let raw = runner.run(&prompt, model).await?;
    let re = Regex::new(r"<answer>(.+?)</answer>").unwrap();
    let json_str = re
        .captures(&raw)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .ok_or_else(|| JudgeError::Malformed(format!("no <answer> in: {}", truncate(&raw, 200))))?;

    let parsed: JudgeAnswer = serde_json::from_str(json_str)
        .map_err(|e| JudgeError::Malformed(format!("bad json: {} ({})", json_str, e)))?;

    Ok(if parsed.complete {
        Verdict::Complete
    } else {
        Verdict::StayInStage
    })
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut i = max;
    while !s.is_char_boundary(i) {
        i -= 1;
    }
    format!("{}...", &s[..i])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::ir::*;
    use async_trait::async_trait;
    use std::collections::BTreeMap;

    struct MockRunner {
        reply: String,
    }

    #[async_trait]
    impl JudgeRunner for MockRunner {
        async fn run(&self, _prompt: &str, _model: &str) -> Result<String, JudgeError> {
            Ok(self.reply.clone())
        }
    }

    fn stage_with_criteria() -> Stage {
        Stage {
            id: "a".into(),
            goal: "Build it".into(),
            prompt: "p".into(),
            completion: Completion::Criteria { criteria: "PRs created".into() },
            next: NextRef::Done,
            notes: Notes::Static,
        }
    }

    fn run_at(stage: &str) -> WorkflowRun {
        WorkflowRun {
            run_id: "r".into(),
            session_id: "s".into(),
            workflow_id: "w".into(),
            inputs: BTreeMap::new(),
            current_stage_id: stage.into(),
            status: crate::runtime::run::RunStatus::Running,
            started_at: 0,
            ended_at: None,
        }
    }

    fn empty_tail() -> Tail {
        Tail { entries: vec![] }
    }

    #[tokio::test]
    async fn parses_complete_true() {
        let runner = MockRunner {
            reply: r#"thinking... <answer>{"complete": true, "reason": "ok"}</answer>"#.into(),
        };
        let v = judge_completion(&runner, "haiku", &stage_with_criteria(), &run_at("a"), &empty_tail())
            .await
            .unwrap();
        assert_eq!(v, Verdict::Complete);
    }

    #[tokio::test]
    async fn parses_complete_false() {
        let runner = MockRunner {
            reply: r#"<answer>{"complete": false, "reason": "no PRs yet"}</answer>"#.into(),
        };
        let v = judge_completion(&runner, "haiku", &stage_with_criteria(), &run_at("a"), &empty_tail())
            .await
            .unwrap();
        assert_eq!(v, Verdict::StayInStage);
    }

    #[tokio::test]
    async fn malformed_response_errors() {
        let runner = MockRunner {
            reply: "no answer block".into(),
        };
        let err = judge_completion(&runner, "haiku", &stage_with_criteria(), &run_at("a"), &empty_tail())
            .await
            .unwrap_err();
        assert!(matches!(err, JudgeError::Malformed(_)));
    }

    #[test]
    fn truncate_does_not_panic_on_multibyte_boundary() {
        let s = "🦀🦀🦀🦀🦀";
        let out = truncate(s, 7);
        assert!(out.ends_with("..."));
        assert!(out.contains("🦀"));
    }
}
