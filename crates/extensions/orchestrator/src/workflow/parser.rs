//! Parses a workflow Markdown file into the IR.
//!
//! Format:
//!   ---
//!   <YAML frontmatter>
//!   ---
//!
//!   ## Stage: <id>
//!   <YAML body for the stage>
//!
//!   ## Stage: <id>
//!   ...

use crate::workflow::ir::*;
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("missing frontmatter delimiter (--- ... ---)")]
    NoFrontmatter,
    #[error("invalid frontmatter YAML: {0}")]
    InvalidFrontmatter(String),
    #[error("invalid stage `{0}` body: {1}")]
    InvalidStage(String, String),
    #[error("workflow has no stages")]
    NoStages,
    #[error("duplicate stage id `{0}`")]
    DuplicateStage(String),
}

pub fn parse(src: &str) -> Result<Workflow, ParseError> {
    let (frontmatter, body) = split_frontmatter(src)?;
    let fm: FrontMatter = serde_yaml::from_str(frontmatter)
        .map_err(|e| ParseError::InvalidFrontmatter(e.to_string()))?;

    let mut stages = BTreeMap::new();
    let mut stage_order = Vec::new();

    for (id, stage_yaml) in split_stages(body) {
        if stages.contains_key(&id) {
            return Err(ParseError::DuplicateStage(id));
        }
        let stage_body: StageBody = serde_yaml::from_str(&stage_yaml)
            .map_err(|e| ParseError::InvalidStage(id.clone(), e.to_string()))?;
        let stage = Stage {
            id: id.clone(),
            goal: stage_body.goal.unwrap_or_default(),
            prompt: stage_body.prompt,
            completion: stage_body.completion,
            next: stage_body.next,
            notes: stage_body.notes.unwrap_or_default(),
        };
        stage_order.push(id.clone());
        stages.insert(id, stage);
    }

    if stages.is_empty() {
        return Err(ParseError::NoStages);
    }

    Ok(Workflow {
        id: fm.id,
        name: fm.name,
        description: fm.description.unwrap_or_default(),
        model: fm.model.unwrap_or_else(|| "claude-haiku-4-5".to_string()),
        inputs: fm.inputs.unwrap_or_default(),
        stages,
        stage_order,
    })
}

#[derive(serde::Deserialize)]
struct FrontMatter {
    id: String,
    name: String,
    description: Option<String>,
    model: Option<String>,
    inputs: Option<Vec<InputSpec>>,
}

#[derive(serde::Deserialize)]
struct StageBody {
    goal: Option<String>,
    prompt: String,
    completion: Completion,
    next: NextRef,
    notes: Option<Notes>,
}

fn split_frontmatter(src: &str) -> Result<(&str, &str), ParseError> {
    let trimmed = src.trim_start();
    let rest = trimmed.strip_prefix("---").ok_or(ParseError::NoFrontmatter)?;
    let rest = rest.trim_start_matches('\n');

    // Find a line that is exactly "---" (no trailing chars before the next \n).
    let mut search_pos = 0;
    let end = loop {
        let needle = rest[search_pos..]
            .find("\n---")
            .ok_or(ParseError::NoFrontmatter)?;
        let abs = search_pos + needle;
        let after_dashes = abs + 4; // index after "\n---"
        let next_byte = rest.as_bytes().get(after_dashes);
        if next_byte.is_none() || next_byte == Some(&b'\n') {
            break abs;
        }
        search_pos = abs + 1;
    };

    let frontmatter = &rest[..end];
    let body = rest[end..].trim_start_matches("\n---").trim_start_matches('\n');
    Ok((frontmatter, body))
}

fn split_stages(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut current_id: Option<String> = None;
    let mut current_buf = String::new();

    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("## Stage:") {
            if let Some(id) = current_id.take() {
                out.push((id, std::mem::take(&mut current_buf)));
            }
            current_id = Some(rest.trim().to_string());
        } else if current_id.is_some() {
            current_buf.push_str(line);
            current_buf.push('\n');
        }
    }
    if let Some(id) = current_id {
        out.push((id, current_buf));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"---
id: ship
name: Ship a feature
description: end-to-end
inputs:
  - name: task
    description: What to ship
    required: true
    type: string
---

## Stage: implement
goal: Build it.
prompt: |
  Please complete {{task}}.
completion:
  type: criteria
  criteria: PRs created.
next: review

## Stage: review
prompt: |
  Review the PRs.
completion:
  type: criteria
  criteria: Reviews done.
next: done
"#;

    #[test]
    fn parses_two_stage_workflow() {
        let wf = parse(SAMPLE).unwrap();
        assert_eq!(wf.id, "ship");
        assert_eq!(wf.stage_order, vec!["implement", "review"]);
        assert_eq!(wf.inputs.len(), 1);
        assert_eq!(wf.inputs[0].name, "task");

        let implement = &wf.stages["implement"];
        assert!(implement.prompt.contains("complete {{task}}"));
        match &implement.completion {
            Completion::Criteria { criteria } => assert!(criteria.contains("PRs created")),
            _ => panic!("expected criteria"),
        }

        match &wf.stages["review"].next {
            NextRef::Done => {}
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn rejects_missing_frontmatter() {
        let err = parse("# Plain markdown").unwrap_err();
        assert!(matches!(err, ParseError::NoFrontmatter));
    }

    #[test]
    fn rejects_no_stages() {
        let src = "---\nid: x\nname: y\n---\n";
        let err = parse(src).unwrap_err();
        assert!(matches!(err, ParseError::NoStages));
    }

    #[test]
    fn rejects_duplicate_stage_ids() {
        let src = r#"---
id: x
name: y
---

## Stage: a
prompt: hi
completion:
  type: criteria
  criteria: c
next: done

## Stage: a
prompt: hi
completion:
  type: criteria
  criteria: c
next: done
"#;
        let err = parse(src).unwrap_err();
        assert!(matches!(err, ParseError::DuplicateStage(ref s) if s == "a"));
    }

    #[test]
    fn frontmatter_does_not_split_on_dashes_in_value() {
        let src = r#"---
id: a
name: b
description: "uses --- inline"
---

## Stage: only
prompt: x
completion:
  type: criteria
  criteria: c
next: done
"#;
        let wf = parse(src).unwrap();
        assert_eq!(wf.id, "a");
        assert!(wf.description.contains("inline"));
    }

    #[test]
    fn stage_prompt_preserves_indentation() {
        let src = r#"---
id: x
name: y
---

## Stage: only
prompt: |
  Line one.
    Indented line.
  Line three.
completion:
  type: criteria
  criteria: c
next: done
"#;
        let wf = parse(src).unwrap();
        let p = &wf.stages["only"].prompt;
        assert!(p.contains("Line one."), "got: {p:?}");
        assert!(p.contains("  Indented line."), "indentation lost: {p:?}");
        assert!(p.contains("Line three."), "got: {p:?}");
    }
}
