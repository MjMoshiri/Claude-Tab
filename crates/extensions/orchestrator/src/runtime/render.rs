use crate::workflow::ir::{Notes, Stage};
use regex::Regex;
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum RenderError {
    #[error("missing input variable `{0}`")]
    MissingInput(String),
}

pub fn render(
    stage: &Stage,
    inputs: &BTreeMap<String, serde_json::Value>,
    dynamic_notes: Option<&str>,
) -> Result<String, RenderError> {
    let body = substitute(&stage.prompt, inputs)?;

    let addendum = match (&stage.notes, dynamic_notes) {
        (Notes::Dynamic { .. }, Some(note)) if !note.is_empty() => {
            format!("\n\n---\n[Orchestrator note: {}]", note)
        }
        _ => String::new(),
    };

    Ok(format!("{}{}", body, addendum))
}

pub fn substitute(
    template: &str,
    inputs: &BTreeMap<String, serde_json::Value>,
) -> Result<String, RenderError> {
    let re = Regex::new(r"\{\{\s*([a-zA-Z_][a-zA-Z0-9_]*)\s*\}\}").unwrap();
    let mut out = String::new();
    let mut last = 0;
    for cap in re.captures_iter(template) {
        let m = cap.get(0).unwrap();
        out.push_str(&template[last..m.start()]);
        let var = &cap[1];
        let v = inputs.get(var).ok_or_else(|| RenderError::MissingInput(var.into()))?;
        out.push_str(&value_to_string(v));
        last = m.end();
    }
    out.push_str(&template[last..]);
    Ok(out)
}

fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::ir::*;

    fn stage(prompt: &str, notes: Notes) -> Stage {
        Stage {
            id: "a".into(),
            goal: String::new(),
            prompt: prompt.into(),
            completion: Completion::Criteria { criteria: "ok".into() },
            next: NextRef::Done,
            notes,
        }
    }

    #[test]
    fn substitutes_string_input() {
        let mut inputs = BTreeMap::new();
        inputs.insert("task".into(), serde_json::Value::String("ship X".into()));
        let s = stage("do {{task}}", Notes::Static);
        assert_eq!(render(&s, &inputs, None).unwrap(), "do ship X");
    }

    #[test]
    fn substitutes_integer_input() {
        let mut inputs = BTreeMap::new();
        inputs.insert("n".into(), serde_json::Value::Number(serde_json::Number::from(3)));
        let s = stage("count {{n}}", Notes::Static);
        assert_eq!(render(&s, &inputs, None).unwrap(), "count 3");
    }

    #[test]
    fn missing_input_errors() {
        let inputs = BTreeMap::new();
        let s = stage("do {{task}}", Notes::Static);
        let err = render(&s, &inputs, None).unwrap_err();
        assert_eq!(err, RenderError::MissingInput("task".into()));
    }

    #[test]
    fn dynamic_notes_appended() {
        let mut inputs = BTreeMap::new();
        inputs.insert("task".into(), serde_json::Value::String("X".into()));
        let s = stage("do {{task}}", Notes::Dynamic { hint: "h".into() });
        let out = render(&s, &inputs, Some("remember docs")).unwrap();
        assert!(out.contains("[Orchestrator note: remember docs]"));
    }

    #[test]
    fn static_notes_skip_addendum_even_if_provided() {
        let mut inputs = BTreeMap::new();
        inputs.insert("task".into(), serde_json::Value::String("X".into()));
        let s = stage("do {{task}}", Notes::Static);
        let out = render(&s, &inputs, Some("ignored")).unwrap();
        assert_eq!(out, "do X");
    }
}
