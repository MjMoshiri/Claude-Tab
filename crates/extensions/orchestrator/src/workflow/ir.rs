use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub inputs: Vec<InputSpec>,
    pub stages: BTreeMap<String, Stage>,
    pub stage_order: Vec<String>,
}

fn default_model() -> String {
    "claude-haiku-4-5".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputSpec {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default = "default_input_type")]
    pub r#type: String, // "string" | "integer" | "bool" | "enum" | "multiline_string"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub r#enum: Vec<String>,
}

fn default_input_type() -> String {
    "string".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stage {
    pub id: String,
    #[serde(default)]
    pub goal: String,
    pub prompt: String,
    pub completion: Completion,
    pub next: NextRef,
    #[serde(default)]
    pub notes: Notes,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Completion {
    Criteria { criteria: String },
    ToolUseMatch {
        #[serde(rename = "match")]
        matcher: ToolUseMatcher,
    },
    SessionEvent { on: SessionEventKind },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolUseMatcher {
    pub tool: String,
    #[serde(default)]
    pub input: BTreeMap<String, String>, // field name → regex
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionEventKind {
    Compact,
    Clear,
    Resume,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NextRef {
    StageId(String),
    Done,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Notes {
    #[default]
    Static,
    Dynamic { hint: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_round_trip() {
        let mut stages = BTreeMap::new();
        stages.insert(
            "implement".to_string(),
            Stage {
                id: "implement".to_string(),
                goal: "Build it".to_string(),
                prompt: "do {{task}}".to_string(),
                completion: Completion::Criteria {
                    criteria: "PRs created".to_string(),
                },
                next: NextRef::Done,
                notes: Notes::Static,
            },
        );
        let wf = Workflow {
            id: "ship".to_string(),
            name: "Ship feature".to_string(),
            description: String::new(),
            model: "claude-haiku-4-5".to_string(),
            inputs: vec![InputSpec {
                name: "task".to_string(),
                description: "What to ship".to_string(),
                required: true,
                r#type: "string".to_string(),
                default: None,
                r#enum: vec![],
            }],
            stages,
            stage_order: vec!["implement".to_string()],
        };

        let json = serde_json::to_string(&wf).unwrap();
        let back: Workflow = serde_json::from_str(&json).unwrap();
        assert_eq!(wf, back);
    }

    #[test]
    fn completion_variants_serialize_with_type_tag() {
        let c = Completion::SessionEvent { on: SessionEventKind::Compact };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("\"type\":\"session_event\""));
        assert!(json.contains("\"on\":\"compact\""));
    }
}
