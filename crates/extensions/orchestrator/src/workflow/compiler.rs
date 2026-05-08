use crate::workflow::ir::*;
use regex::Regex;
use std::collections::HashSet;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum CompileError {
    #[error("stage `{0}` references unknown stage `{1}` via next")]
    UnknownStage(String, String),
    #[error("stage `{0}` references undeclared input `{1}`")]
    UnknownVar(String, String),
    #[error("workflow has no reachable `done` terminal")]
    NoTerminal,
    #[error("workflow has duplicate stage id `{0}`")]
    DuplicateStage(String),
    #[error("stage `{0}` has unsupported completion configuration: {1}")]
    UnsupportedCompletion(String, String),
}

pub fn compile(wf: &Workflow) -> Result<(), CompileError> {
    let var_re = Regex::new(r"\{\{\s*([a-zA-Z_][a-zA-Z0-9_]*)\s*\}\}").unwrap();
    let declared_inputs: HashSet<&str> = wf.inputs.iter().map(|i| i.name.as_str()).collect();

    let stage_ids: HashSet<&str> = wf.stages.keys().map(String::as_str).collect();
    if stage_ids.len() != wf.stage_order.len() {
        // BTreeMap<String, Stage> already de-dupes, but stage_order may carry dupes
        let mut seen = HashSet::new();
        for id in &wf.stage_order {
            if !seen.insert(id) {
                return Err(CompileError::DuplicateStage(id.clone()));
            }
        }
    }

    // Validate next refs and {{var}} references
    for (id, stage) in &wf.stages {
        if let NextRef::StageId(next_id) = &stage.next {
            if !stage_ids.contains(next_id.as_str()) {
                return Err(CompileError::UnknownStage(id.clone(), next_id.clone()));
            }
        }
        let mut fields = vec![&stage.prompt];
        if let Notes::Dynamic { hint } = &stage.notes {
            fields.push(hint);
        }
        if let Completion::Criteria { criteria } = &stage.completion {
            fields.push(criteria);
        }
        for field in fields {
            for cap in var_re.captures_iter(field) {
                let var = &cap[1];
                if !declared_inputs.contains(var) {
                    return Err(CompileError::UnknownVar(id.clone(), var.to_string()));
                }
            }
        }
    }

    // Reachability: BFS from stage_order[0], must hit `done`
    if wf.stage_order.is_empty() {
        return Err(CompileError::NoTerminal);
    }
    let start = &wf.stage_order[0];
    let mut visited = HashSet::new();
    let mut queue = vec![start.clone()];
    let mut reaches_done = false;

    while let Some(id) = queue.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let stage = &wf.stages[&id];
        match &stage.next {
            NextRef::Done => reaches_done = true,
            NextRef::StageId(next_id) => queue.push(next_id.clone()),
        }
    }

    if !reaches_done {
        return Err(CompileError::NoTerminal);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn make_wf(stages: Vec<(&str, NextRef, Completion, Vec<&str>)>, inputs: Vec<&str>) -> Workflow {
        let mut map = BTreeMap::new();
        let mut order = Vec::new();
        for (id, next, completion, _vars) in &stages {
            map.insert(
                id.to_string(),
                Stage {
                    id: id.to_string(),
                    goal: String::new(),
                    prompt: format!("do {{{{{}}}}}", _vars.first().copied().unwrap_or("nothing")),
                    completion: completion.clone(),
                    next: next.clone(),
                    notes: Notes::Static,
                },
            );
            order.push(id.to_string());
        }
        Workflow {
            id: "test".into(),
            name: "test".into(),
            description: String::new(),
            model: "claude-haiku-4-5".into(),
            inputs: inputs.into_iter().map(|n| InputSpec {
                name: n.into(),
                description: String::new(),
                required: true,
                r#type: "string".into(),
                default: None,
                r#enum: vec![],
            }).collect(),
            stages: map,
            stage_order: order,
        }
    }

    fn crit() -> Completion {
        Completion::Criteria { criteria: "ok".into() }
    }

    #[test]
    fn valid_linear_workflow_compiles() {
        let wf = make_wf(
            vec![
                ("a", NextRef::StageId("b".into()), crit(), vec!["task"]),
                ("b", NextRef::Done, crit(), vec!["task"]),
            ],
            vec!["task"],
        );
        compile(&wf).unwrap();
    }

    #[test]
    fn unknown_next_stage_rejected() {
        let wf = make_wf(
            vec![("a", NextRef::StageId("ghost".into()), crit(), vec!["task"])],
            vec!["task"],
        );
        let err = compile(&wf).unwrap_err();
        assert!(matches!(err, CompileError::UnknownStage(_, _)));
    }

    #[test]
    fn undeclared_var_rejected() {
        let wf = make_wf(
            vec![("a", NextRef::Done, crit(), vec!["mystery"])],
            vec!["task"],
        );
        let err = compile(&wf).unwrap_err();
        assert!(matches!(err, CompileError::UnknownVar(_, _)));
    }

    #[test]
    fn no_terminal_rejected() {
        // Cycle a → b → a, never reaches done
        let wf = make_wf(
            vec![
                ("a", NextRef::StageId("b".into()), crit(), vec!["task"]),
                ("b", NextRef::StageId("a".into()), crit(), vec!["task"]),
            ],
            vec!["task"],
        );
        let err = compile(&wf).unwrap_err();
        assert_eq!(err, CompileError::NoTerminal);
    }
}
