# Workflow Orchestration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers-extended-cc:subagent-driven-development (recommended) or superpowers-extended-cc:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship v1 workflow orchestration: Claude-Tab attaches a Markdown-defined workflow to a Claude Code session and advances it through stages automatically by judging completion via `claude -p`, injecting next-stage prompts via PTY auto-typing.

**Architecture:** Hybrid FSM rails + LLM judges. Plugin (`claude-tabs-orchestrator`) emits Stop/SessionStart hooks → POST localhost HTTP to a Tab-hosted axum server → orchestrator extension judges completion → advances run state in SQLite → PTY-types next prompt. See `docs/superpowers/specs/2026-05-07-workflow-orchestration-design.md` for the full spec.

**Tech Stack:** Rust (extension crate, axum, rusqlite, tokio, portable-pty), TypeScript/React (Tab UI), Bash (plugin hook scripts), `claude -p` subprocess for LLM judges.

**Phases:**
1. Backend foundation — Tasks 0–10 (parser, compiler, FSM, store, render, transcript, judges, injector)
2. Transport + plugin — Tasks 11–14 (HTTP, auth, plugin scripts)
3. Frontend + release — Tasks 15–21 (UI, feature flag, docs, smoke test)

Each task is one commit. TDD inside each task.

---

## Task 0: Crate scaffolding & workspace registration

**Goal:** Create empty `claude-tabs-ext-orchestrator` crate; register in workspace; assert it builds.

**Files:**
- Create: `crates/extensions/orchestrator/Cargo.toml`
- Create: `crates/extensions/orchestrator/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

**Acceptance Criteria:**
- [ ] `cargo build -p claude-tabs-ext-orchestrator` succeeds
- [ ] Empty `Extension` impl present for registration

**Verify:** `cargo build -p claude-tabs-ext-orchestrator` → no errors

**Steps:**

- [ ] **Step 1: Add workspace member**

In root `Cargo.toml`, add `"crates/extensions/orchestrator",` to `[workspace] members`.

- [ ] **Step 2: Create `crates/extensions/orchestrator/Cargo.toml`**

```toml
[package]
name = "claude-tabs-ext-orchestrator"
version = "0.1.0"
authors = ["MohammadJavad Moshiri"]
license = "AGPL-3.0-or-later"
edition = "2021"

[dependencies]
claude-tabs-core = { path = "../../core" }
claude-tabs-storage = { path = "../../storage" }
claude-tabs-pty = { path = "../../pty" }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
async-trait = { workspace = true }
thiserror = { workspace = true }
tracing = { workspace = true }
chrono = { workspace = true }
uuid = { workspace = true }
rusqlite = { workspace = true }
regex = { workspace = true }
axum = "0.7"
tower = "0.4"
tower-http = { version = "0.5", features = ["trace"] }
sha2 = "0.10"
hex = "0.4"

[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Create `crates/extensions/orchestrator/src/lib.rs`**

```rust
//! Workflow orchestration extension.
//!
//! Drives a Claude Code session through pre-authored workflow stages
//! by judging stage completion via the `claude -p` CLI and PTY-typing
//! the next stage's prompt when the current stage is done.

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
```

- [ ] **Step 4: Build and commit**

```bash
cargo build -p claude-tabs-ext-orchestrator
git add crates/extensions/orchestrator Cargo.toml
git commit -m "feat(orchestrator): scaffold orchestrator extension crate"
```

---

## Task 1: SQLite schema migration

**Goal:** Add `workflows`, `workflow_runs`, `stage_history` tables to the existing `archive.db` migration.

**Files:**
- Modify: `crates/storage/src/migrations.rs`
- Test: `crates/storage/src/migrations.rs` (inline `#[cfg(test)]`)

**Acceptance Criteria:**
- [ ] Migration runs idempotently on a fresh DB and an existing DB
- [ ] All three tables + indexes exist after migration
- [ ] Existing tables (`directory_preferences`, `session_metadata`) untouched

**Verify:** `cargo test -p claude-tabs-storage migrations` → PASS

**Steps:**

- [ ] **Step 1: Write the failing migration test**

Add to bottom of `crates/storage/src/migrations.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn migration_creates_orchestrator_tables() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        assert!(tables.contains(&"workflows".to_string()));
        assert!(tables.contains(&"workflow_runs".to_string()));
        assert!(tables.contains(&"stage_history".to_string()));
    }

    #[test]
    fn migration_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();
    }
}
```

- [ ] **Step 2: Run the test and confirm it fails**

```bash
cargo test -p claude-tabs-storage migration_creates_orchestrator_tables
```

Expected: FAIL — tables not present.

- [ ] **Step 3: Add migration body**

In `crates/storage/src/migrations.rs`, append to the `execute_batch` block:

```rust
conn.execute_batch(
    "
    CREATE TABLE IF NOT EXISTS workflows (
      id            TEXT PRIMARY KEY,
      name          TEXT NOT NULL,
      description   TEXT,
      source_path   TEXT NOT NULL,
      source_hash   TEXT NOT NULL,
      compiled_json TEXT NOT NULL,
      updated_at    INTEGER NOT NULL
    );

    CREATE TABLE IF NOT EXISTS workflow_runs (
      run_id            TEXT PRIMARY KEY,
      session_id        TEXT NOT NULL UNIQUE,
      workflow_id       TEXT NOT NULL REFERENCES workflows(id),
      inputs_json       TEXT NOT NULL,
      current_stage_id  TEXT NOT NULL,
      status            TEXT NOT NULL,
      started_at        INTEGER NOT NULL,
      ended_at          INTEGER
    );
    CREATE INDEX IF NOT EXISTS idx_runs_session ON workflow_runs(session_id);

    CREATE TABLE IF NOT EXISTS stage_history (
      id          INTEGER PRIMARY KEY AUTOINCREMENT,
      run_id      TEXT NOT NULL REFERENCES workflow_runs(run_id),
      stage_id    TEXT NOT NULL,
      entered_at  INTEGER NOT NULL,
      exited_at   INTEGER,
      verdict     TEXT,
      judge_log   TEXT
    );
    CREATE INDEX IF NOT EXISTS idx_history_run ON stage_history(run_id);
    ",
)?;
```

⚠️ NOTE: existing migration has `DROP TABLE IF EXISTS` at the top for old tables. Do NOT add the new tables to that drop block — they should persist across restarts.

- [ ] **Step 4: Run all storage tests**

```bash
cargo test -p claude-tabs-storage
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/storage/src/migrations.rs
git commit -m "feat(storage): add workflow orchestrator tables"
```

---

## Task 2: Workflow IR types

**Goal:** Define the in-memory Rust IR for a parsed/compiled workflow, with serde support.

**Files:**
- Create: `crates/extensions/orchestrator/src/workflow/mod.rs`
- Create: `crates/extensions/orchestrator/src/workflow/ir.rs`
- Modify: `crates/extensions/orchestrator/src/lib.rs`

**Acceptance Criteria:**
- [ ] `Workflow`, `Stage`, `Completion`, `InputSpec`, `Notes` structs serialize/deserialize round-trip
- [ ] `serde_json` round-trip test passes

**Verify:** `cargo test -p claude-tabs-ext-orchestrator workflow::ir` → PASS

**Steps:**

- [ ] **Step 1: Write IR types**

Create `crates/extensions/orchestrator/src/workflow/mod.rs`:

```rust
pub mod ir;
```

Create `crates/extensions/orchestrator/src/workflow/ir.rs`:

```rust
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
```

- [ ] **Step 2: Add to lib.rs**

In `crates/extensions/orchestrator/src/lib.rs` add at top:

```rust
pub mod workflow;
```

- [ ] **Step 3: Write round-trip test**

Create `crates/extensions/orchestrator/src/workflow/ir.rs` test module at the bottom:

```rust
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
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p claude-tabs-ext-orchestrator workflow::ir
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/extensions/orchestrator/
git commit -m "feat(orchestrator): workflow IR types with serde round-trip"
```

---

## Task 3: Markdown parser

**Goal:** Parse a workflow Markdown file (frontmatter + `## Stage:` blocks) into the IR.

**Files:**
- Create: `crates/extensions/orchestrator/src/workflow/parser.rs`
- Modify: `crates/extensions/orchestrator/src/workflow/mod.rs`
- Modify: `crates/extensions/orchestrator/Cargo.toml` (add `serde_yaml`)

**Acceptance Criteria:**
- [ ] Parses a 3-stage example workflow correctly
- [ ] Stage prompts preserve verbatim whitespace inside YAML `prompt: |` blocks
- [ ] Returns descriptive errors on malformed frontmatter or missing required fields

**Verify:** `cargo test -p claude-tabs-ext-orchestrator workflow::parser` → PASS

**Steps:**

- [ ] **Step 1: Add YAML dep**

In `crates/extensions/orchestrator/Cargo.toml` `[dependencies]`:

```toml
serde_yaml = "0.9"
```

- [ ] **Step 2: Add module**

`crates/extensions/orchestrator/src/workflow/mod.rs`:

```rust
pub mod ir;
pub mod parser;
```

- [ ] **Step 3: Write parser tests first**

Create `crates/extensions/orchestrator/src/workflow/parser.rs`:

```rust
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
    #[error("missing required field `{0}`")]
    MissingField(&'static str),
}

pub fn parse(src: &str) -> Result<Workflow, ParseError> {
    let (frontmatter, body) = split_frontmatter(src)?;
    let fm: FrontMatter = serde_yaml::from_str(frontmatter)
        .map_err(|e| ParseError::InvalidFrontmatter(e.to_string()))?;

    let mut stages = BTreeMap::new();
    let mut stage_order = Vec::new();

    for (id, stage_yaml) in split_stages(body) {
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
    let end = rest.find("\n---").ok_or(ParseError::NoFrontmatter)?;
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
        matches!(err, ParseError::NoFrontmatter);
    }

    #[test]
    fn rejects_no_stages() {
        let src = "---\nid: x\nname: y\n---\n";
        let err = parse(src).unwrap_err();
        matches!(err, ParseError::NoStages);
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p claude-tabs-ext-orchestrator workflow::parser
```

Expected: PASS (3 tests)

- [ ] **Step 5: Commit**

```bash
git add crates/extensions/orchestrator/ Cargo.lock
git commit -m "feat(orchestrator): parse workflow Markdown into IR"
```

---

## Task 4: Workflow compiler

**Goal:** Validate a parsed `Workflow`: stage refs resolve, `{{vars}}` declared, exactly one path to `done`, no duplicate stage ids.

**Files:**
- Create: `crates/extensions/orchestrator/src/workflow/compiler.rs`
- Modify: `crates/extensions/orchestrator/src/workflow/mod.rs`

**Acceptance Criteria:**
- [ ] Valid workflow → `Ok(())`
- [ ] Workflow with `next: missing_id` → `Err(UnknownStage)`
- [ ] Workflow referencing `{{undeclared}}` → `Err(UnknownVar)`
- [ ] Workflow with no path to `done` → `Err(NoTerminal)`

**Verify:** `cargo test -p claude-tabs-ext-orchestrator workflow::compiler` → PASS

**Steps:**

- [ ] **Step 1: Add module**

`crates/extensions/orchestrator/src/workflow/mod.rs`:

```rust
pub mod ir;
pub mod parser;
pub mod compiler;
```

- [ ] **Step 2: Write tests + impl together**

Create `crates/extensions/orchestrator/src/workflow/compiler.rs`:

```rust
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

    // Validate next refs and collect reachability
    for (id, stage) in &wf.stages {
        if let NextRef::StageId(next_id) = &stage.next {
            if !stage_ids.contains(next_id.as_str()) {
                return Err(CompileError::UnknownStage(id.clone(), next_id.clone()));
            }
        }
        // Validate {{var}} references in fields that template
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
```

- [ ] **Step 3: Run tests**

```bash
cargo test -p claude-tabs-ext-orchestrator workflow::compiler
```

Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/extensions/orchestrator/src/workflow/
git commit -m "feat(orchestrator): compile workflow IR with reachability + var checks"
```

---

## Task 5: Workflow loader (scan + cache)

**Goal:** Scan `~/.claude-tabs/workflows/*.md`, parse + compile, persist into `workflows` SQLite table when source hash changes; expose loaded workflows via an in-memory `WorkflowRegistry`.

**Files:**
- Create: `crates/extensions/orchestrator/src/workflow/loader.rs`
- Create: `crates/extensions/orchestrator/src/workflow/registry.rs`
- Modify: `crates/extensions/orchestrator/src/workflow/mod.rs`

**Acceptance Criteria:**
- [ ] Loader returns one entry per valid `.md` file
- [ ] Bad files surface a `LoadError` per file but don't poison others
- [ ] On second run with unchanged file, `compile()` is skipped (hash check)
- [ ] `WorkflowRegistry::get(id)` returns `Some(&Workflow)` for compiled workflows

**Verify:** `cargo test -p claude-tabs-ext-orchestrator workflow::loader` → PASS

**Steps:**

- [ ] **Step 1: Add modules**

```rust
// workflow/mod.rs
pub mod ir;
pub mod parser;
pub mod compiler;
pub mod loader;
pub mod registry;
```

- [ ] **Step 2: Implement registry**

`crates/extensions/orchestrator/src/workflow/registry.rs`:

```rust
use crate::workflow::ir::Workflow;
use std::collections::HashMap;
use std::sync::RwLock;

pub struct WorkflowRegistry {
    inner: RwLock<HashMap<String, Workflow>>,
}

impl WorkflowRegistry {
    pub fn new() -> Self {
        Self { inner: RwLock::new(HashMap::new()) }
    }

    pub fn upsert(&self, wf: Workflow) {
        self.inner.write().unwrap().insert(wf.id.clone(), wf);
    }

    pub fn get(&self, id: &str) -> Option<Workflow> {
        self.inner.read().unwrap().get(id).cloned()
    }

    pub fn list(&self) -> Vec<Workflow> {
        self.inner.read().unwrap().values().cloned().collect()
    }
}

impl Default for WorkflowRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 3: Implement loader with tests**

`crates/extensions/orchestrator/src/workflow/loader.rs`:

```rust
use crate::workflow::compiler::{compile, CompileError};
use crate::workflow::ir::Workflow;
use crate::workflow::parser::{parse, ParseError};
use crate::workflow::registry::WorkflowRegistry;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("io error reading {0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("parse error in {0}: {1}")]
    Parse(PathBuf, ParseError),
    #[error("compile error in {0}: {1}")]
    Compile(PathBuf, CompileError),
    #[error("file too large: {0} ({1} bytes, max 256KB)")]
    TooLarge(PathBuf, u64),
}

pub struct LoadResult {
    pub loaded: Vec<(Workflow, String)>, // (workflow, source_hash)
    pub errors: Vec<LoadError>,
}

const MAX_FILE_BYTES: u64 = 256 * 1024;

pub fn scan_directory(dir: &Path) -> LoadResult {
    let mut loaded = Vec::new();
    let mut errors = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            errors.push(LoadError::Io(dir.to_path_buf(), e));
            return LoadResult { loaded, errors };
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        match load_one(&path) {
            Ok(pair) => loaded.push(pair),
            Err(e) => errors.push(e),
        }
    }

    LoadResult { loaded, errors }
}

pub fn load_one(path: &Path) -> Result<(Workflow, String), LoadError> {
    let meta = std::fs::metadata(path).map_err(|e| LoadError::Io(path.to_path_buf(), e))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(LoadError::TooLarge(path.to_path_buf(), meta.len()));
    }
    let src = std::fs::read_to_string(path).map_err(|e| LoadError::Io(path.to_path_buf(), e))?;
    let wf = parse(&src).map_err(|e| LoadError::Parse(path.to_path_buf(), e))?;
    compile(&wf).map_err(|e| LoadError::Compile(path.to_path_buf(), e))?;

    let hash = hex::encode(Sha256::digest(src.as_bytes()));
    Ok((wf, hash))
}

pub fn populate_registry(dir: &Path, reg: &WorkflowRegistry) -> Vec<LoadError> {
    let result = scan_directory(dir);
    for (wf, _hash) in result.loaded {
        reg.upsert(wf);
    }
    result.errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &Path, name: &str, contents: &str) {
        std::fs::write(dir.join(name), contents).unwrap();
    }

    const VALID: &str = r#"---
id: w1
name: Test
inputs:
  - name: task
    description: x
    required: true
    type: string
---

## Stage: a
prompt: |
  do {{task}}
completion:
  type: criteria
  criteria: done
next: done
"#;

    #[test]
    fn loads_valid_workflow() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "w1.md", VALID);
        let result = scan_directory(tmp.path());
        assert_eq!(result.loaded.len(), 1);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn bad_file_does_not_poison_others() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "ok.md", VALID);
        write(tmp.path(), "bad.md", "no frontmatter here");
        let result = scan_directory(tmp.path());
        assert_eq!(result.loaded.len(), 1);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn populates_registry() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "w1.md", VALID);
        let reg = WorkflowRegistry::new();
        let errs = populate_registry(tmp.path(), &reg);
        assert!(errs.is_empty());
        assert!(reg.get("w1").is_some());
    }
}
```

- [ ] **Step 4: Test + commit**

```bash
cargo test -p claude-tabs-ext-orchestrator workflow::loader
git add crates/extensions/orchestrator/src/workflow/
git commit -m "feat(orchestrator): scan + load workflows from ~/.claude-tabs/workflows"
```

NOTE: SQLite cache (Tasks 1's `workflows` table) is wired up in Task 7 (run store) when a `Connection` is plumbed through; for v1 the in-memory registry is sufficient and recompilation on every Tab startup is cheap (workflows are small).

---

## Task 6: WorkflowRun + pure FSM advance

**Goal:** Define `WorkflowRun` state and a pure `advance(run, workflow, verdict) → AdvanceOutcome` function with no I/O.

**Files:**
- Create: `crates/extensions/orchestrator/src/runtime/mod.rs`
- Create: `crates/extensions/orchestrator/src/runtime/run.rs`
- Modify: `crates/extensions/orchestrator/src/lib.rs`

**Acceptance Criteria:**
- [ ] `Verdict::StayInStage` → `Outcome::NoOp`
- [ ] `Verdict::Complete` w/ `next: Done` → `Outcome::Done`
- [ ] `Verdict::Complete` w/ `next: StageId` → `Outcome::Inject { stage_id, ... }`
- [ ] `Verdict::Escalate` → `Outcome::Escalate`

**Verify:** `cargo test -p claude-tabs-ext-orchestrator runtime::run` → PASS

**Steps:**

- [ ] **Step 1: Add module**

`crates/extensions/orchestrator/src/lib.rs`:

```rust
pub mod runtime;
pub mod workflow;
```

`crates/extensions/orchestrator/src/runtime/mod.rs`:

```rust
pub mod run;
```

- [ ] **Step 2: Implement run + advance + tests**

`crates/extensions/orchestrator/src/runtime/run.rs`:

```rust
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
```

- [ ] **Step 3: Test + commit**

```bash
cargo test -p claude-tabs-ext-orchestrator runtime::run
git add crates/extensions/orchestrator/src/
git commit -m "feat(orchestrator): pure FSM advance fn"
```

---

## Task 7: Run store (SQLite I/O)

**Goal:** Persist `WorkflowRun` and `stage_history` rows to `archive.db`. Owns DB connection, exposes async-safe API.

**Files:**
- Create: `crates/extensions/orchestrator/src/runtime/store.rs`
- Modify: `crates/extensions/orchestrator/src/runtime/mod.rs`

**Acceptance Criteria:**
- [ ] `RunStore::create_run`, `load_by_session`, `advance_run`, `record_history`, `set_status` work
- [ ] `UNIQUE(session_id)` violation surfaces as a typed error
- [ ] Round-trip preserves `inputs_json`, `current_stage_id`, `status`

**Verify:** `cargo test -p claude-tabs-ext-orchestrator runtime::store` → PASS

**Steps:**

- [ ] **Step 1: Add module**

`crates/extensions/orchestrator/src/runtime/mod.rs`:

```rust
pub mod run;
pub mod store;
```

- [ ] **Step 2: Implement store**

`crates/extensions/orchestrator/src/runtime/store.rs`:

```rust
use crate::runtime::run::{RunStatus, WorkflowRun};
use chrono::Utc;
use rusqlite::{params, Connection};
use std::collections::BTreeMap;
use std::sync::Mutex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("session already has a workflow run: {0}")]
    SessionAlreadyAttached(String),
    #[error("run not found for session {0}")]
    NotFound(String),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct RunStore {
    conn: Mutex<Connection>,
}

impl RunStore {
    pub fn new(conn: Connection) -> Self {
        Self { conn: Mutex::new(conn) }
    }

    pub fn create_run(&self, run: &WorkflowRun) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let inputs_json = serde_json::to_string(&run.inputs)?;
        let result = conn.execute(
            "INSERT INTO workflow_runs
              (run_id, session_id, workflow_id, inputs_json, current_stage_id, status, started_at, ended_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                run.run_id, run.session_id, run.workflow_id,
                inputs_json, run.current_stage_id,
                run_status_to_str(run.status), run.started_at, run.ended_at,
            ],
        );
        match result {
            Ok(_) => Ok(()),
            Err(rusqlite::Error::SqliteFailure(e, _))
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Err(StoreError::SessionAlreadyAttached(run.session_id.clone()))
            }
            Err(e) => Err(e.into()),
        }
    }

    pub fn load_by_session(&self, session_id: &str) -> Result<Option<WorkflowRun>, StoreError> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT run_id, session_id, workflow_id, inputs_json, current_stage_id,
                    status, started_at, ended_at
             FROM workflow_runs WHERE session_id = ?",
        )?;
        let row = stmt
            .query_row(params![session_id], |row| {
                let inputs_json: String = row.get(3)?;
                let inputs: BTreeMap<String, serde_json::Value> =
                    serde_json::from_str(&inputs_json).unwrap_or_default();
                Ok(WorkflowRun {
                    run_id: row.get(0)?,
                    session_id: row.get(1)?,
                    workflow_id: row.get(2)?,
                    inputs,
                    current_stage_id: row.get(4)?,
                    status: str_to_run_status(&row.get::<_, String>(5)?),
                    started_at: row.get(6)?,
                    ended_at: row.get(7)?,
                })
            });
        match row {
            Ok(r) => Ok(Some(r)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn advance_run(&self, run_id: &str, new_stage_id: &str) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE workflow_runs SET current_stage_id = ? WHERE run_id = ?",
            params![new_stage_id, run_id],
        )?;
        Ok(())
    }

    pub fn set_status(&self, run_id: &str, status: RunStatus) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let ended_at = if matches!(status, RunStatus::Done | RunStatus::Error | RunStatus::Escalated) {
            Some(Utc::now().timestamp_millis())
        } else {
            None
        };
        conn.execute(
            "UPDATE workflow_runs SET status = ?, ended_at = COALESCE(?, ended_at) WHERE run_id = ?",
            params![run_status_to_str(status), ended_at, run_id],
        )?;
        Ok(())
    }

    pub fn record_history(
        &self,
        run_id: &str,
        stage_id: &str,
        verdict: &str,
        judge_log: Option<&str>,
    ) -> Result<(), StoreError> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().timestamp_millis();
        conn.execute(
            "INSERT INTO stage_history (run_id, stage_id, entered_at, verdict, judge_log)
             VALUES (?, ?, ?, ?, ?)",
            params![run_id, stage_id, now, verdict, judge_log],
        )?;
        Ok(())
    }
}

fn run_status_to_str(s: RunStatus) -> &'static str {
    match s {
        RunStatus::Running => "running",
        RunStatus::Paused => "paused",
        RunStatus::Done => "done",
        RunStatus::Escalated => "escalated",
        RunStatus::Error => "error",
    }
}

fn str_to_run_status(s: &str) -> RunStatus {
    match s {
        "paused" => RunStatus::Paused,
        "done" => RunStatus::Done,
        "escalated" => RunStatus::Escalated,
        "error" => RunStatus::Error,
        _ => RunStatus::Running,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claude_tabs_storage::migrations::run_migrations;
    use rusqlite::Connection;

    fn store() -> RunStore {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        // workflows row needs to exist for FK
        conn.execute(
            "INSERT INTO workflows (id, name, source_path, source_hash, compiled_json, updated_at)
             VALUES ('w1', 'W', '/tmp/w1.md', 'abc', '{}', 0)",
            [],
        ).unwrap();
        RunStore::new(conn)
    }

    fn make_run(session: &str) -> WorkflowRun {
        WorkflowRun {
            run_id: format!("run-{session}"),
            session_id: session.into(),
            workflow_id: "w1".into(),
            inputs: BTreeMap::new(),
            current_stage_id: "a".into(),
            status: RunStatus::Running,
            started_at: 1,
            ended_at: None,
        }
    }

    #[test]
    fn create_and_load_round_trip() {
        let s = store();
        let run = make_run("s1");
        s.create_run(&run).unwrap();
        let back = s.load_by_session("s1").unwrap().unwrap();
        assert_eq!(back.current_stage_id, "a");
        assert_eq!(back.status, RunStatus::Running);
    }

    #[test]
    fn duplicate_session_rejected() {
        let s = store();
        let run = make_run("s1");
        s.create_run(&run).unwrap();
        let err = s.create_run(&run).unwrap_err();
        assert!(matches!(err, StoreError::SessionAlreadyAttached(_)));
    }

    #[test]
    fn advance_updates_stage() {
        let s = store();
        let run = make_run("s1");
        s.create_run(&run).unwrap();
        s.advance_run(&run.run_id, "b").unwrap();
        let back = s.load_by_session("s1").unwrap().unwrap();
        assert_eq!(back.current_stage_id, "b");
    }
}
```

- [ ] **Step 3: Make `migrations` public**

In `crates/storage/src/lib.rs`, change `pub mod migrations;` if not already pub. (It is, per the existing scaffold.)

- [ ] **Step 4: Test + commit**

```bash
cargo test -p claude-tabs-ext-orchestrator runtime::store
git add crates/extensions/orchestrator/src/
git commit -m "feat(orchestrator): SQLite-backed run store"
```

---

## Task 8: Render + variable substitution

**Goal:** Substitute `{{var}}` placeholders into stage prompts; append dynamic notes addendum when present.

**Files:**
- Create: `crates/extensions/orchestrator/src/runtime/render.rs`
- Modify: `crates/extensions/orchestrator/src/runtime/mod.rs`

**Acceptance Criteria:**
- [ ] `render(stage, inputs, notes_addendum)` substitutes `{{name}}` from `inputs`
- [ ] Missing input → returns `Err(MissingInput)` (compiler should have caught it but defensive)
- [ ] Static notes → no addendum appended
- [ ] Dynamic notes addendum → appended with `\n\n---\n[Orchestrator note: ...]`

**Verify:** `cargo test -p claude-tabs-ext-orchestrator runtime::render` → PASS

**Steps:**

- [ ] **Step 1: Add module + impl + tests**

`crates/extensions/orchestrator/src/runtime/mod.rs`:

```rust
pub mod render;
pub mod run;
pub mod store;
```

`crates/extensions/orchestrator/src/runtime/render.rs`:

```rust
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
```

- [ ] **Step 2: Test + commit**

```bash
cargo test -p claude-tabs-ext-orchestrator runtime::render
git add crates/extensions/orchestrator/src/
git commit -m "feat(orchestrator): render stage prompts with var substitution + notes"
```

---

## Task 9: Transcript JSONL reader

**Goal:** Open a Claude Code transcript JSONL file, tail the last N entries, expose tool_use entries and last assistant message.

**Files:**
- Create: `crates/extensions/orchestrator/src/transcript.rs`
- Modify: `crates/extensions/orchestrator/src/lib.rs`

**Acceptance Criteria:**
- [ ] `tail(path, n)` reads last N entries (or fewer if file shorter)
- [ ] `tool_uses()` returns only tool_use entries
- [ ] `last_assistant_msg()` returns the last assistant text message
- [ ] Unknown line shapes are skipped, not panicking

**Verify:** `cargo test -p claude-tabs-ext-orchestrator transcript` → PASS

**Steps:**

- [ ] **Step 1: Add module**

`crates/extensions/orchestrator/src/lib.rs` — add `pub mod transcript;`

- [ ] **Step 2: Implement reader**

`crates/extensions/orchestrator/src/transcript.rs`:

```rust
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum Entry {
    #[serde(rename = "user")]
    User { content: serde_json::Value },
    #[serde(rename = "assistant")]
    Assistant { content: serde_json::Value },
    #[serde(rename = "tool_use")]
    ToolUse {
        name: String,
        input: serde_json::Value,
        #[serde(default)]
        id: String,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        #[serde(default)]
        tool_use_id: String,
        content: serde_json::Value,
    },
    #[serde(other)]
    Other,
}

pub struct Tail {
    pub entries: Vec<Entry>,
}

impl Tail {
    pub fn read(path: &Path, n: usize) -> std::io::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let mut entries: Vec<Entry> = content
            .lines()
            .filter_map(|line| serde_json::from_str::<Entry>(line).ok())
            .collect();
        if entries.len() > n {
            entries = entries.split_off(entries.len() - n);
        }
        Ok(Tail { entries })
    }

    pub fn tool_uses(&self) -> impl Iterator<Item = (&str, &serde_json::Value)> + '_ {
        self.entries.iter().filter_map(|e| match e {
            Entry::ToolUse { name, input, .. } => Some((name.as_str(), input)),
            _ => None,
        })
    }

    pub fn last_assistant_text(&self) -> Option<String> {
        for e in self.entries.iter().rev() {
            if let Entry::Assistant { content } = e {
                return Some(extract_text(content));
            }
        }
        None
    }

    pub fn compact_repr(&self, max_chars: usize) -> String {
        let mut out = String::new();
        for e in &self.entries {
            let line = match e {
                Entry::User { content } => format!("[user] {}\n", extract_text(content)),
                Entry::Assistant { content } => format!("[asst] {}\n", extract_text(content)),
                Entry::ToolUse { name, input, .. } => {
                    format!("[tool_use {}] {}\n", name, truncate(input.to_string(), 200))
                }
                Entry::ToolResult { content, .. } => {
                    format!("[tool_result] {}\n", truncate(extract_text(content), 200))
                }
                Entry::Other => continue,
            };
            out.push_str(&line);
            if out.len() > max_chars {
                out.truncate(max_chars);
                out.push_str("\n[...truncated]");
                break;
            }
        }
        out
    }
}

fn extract_text(v: &serde_json::Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    if let Some(arr) = v.as_array() {
        return arr
            .iter()
            .filter_map(|item| {
                item.get("text").and_then(|t| t.as_str()).map(String::from)
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    v.to_string()
}

fn truncate(s: String, max: usize) -> String {
    if s.len() <= max { s } else {
        let mut t = s;
        t.truncate(max);
        t.push_str("...");
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn write_jsonl(lines: &[&str]) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        for l in lines {
            writeln!(f, "{}", l).unwrap();
        }
        f
    }

    #[test]
    fn reads_tail_n_entries() {
        let f = write_jsonl(&[
            r#"{"type":"user","content":"hi"}"#,
            r#"{"type":"assistant","content":"hello"}"#,
            r#"{"type":"tool_use","name":"Bash","input":{"command":"ls"}}"#,
        ]);
        let tail = Tail::read(f.path(), 2).unwrap();
        assert_eq!(tail.entries.len(), 2);
    }

    #[test]
    fn tool_uses_filters_correctly() {
        let f = write_jsonl(&[
            r#"{"type":"user","content":"hi"}"#,
            r#"{"type":"tool_use","name":"Bash","input":{"command":"gh pr create"}}"#,
        ]);
        let tail = Tail::read(f.path(), 10).unwrap();
        let uses: Vec<_> = tail.tool_uses().collect();
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].0, "Bash");
    }

    #[test]
    fn skips_malformed_lines() {
        let f = write_jsonl(&[
            "not json",
            r#"{"type":"assistant","content":"ok"}"#,
        ]);
        let tail = Tail::read(f.path(), 10).unwrap();
        assert_eq!(tail.entries.len(), 1);
    }
}
```

- [ ] **Step 3: Test + commit**

```bash
cargo test -p claude-tabs-ext-orchestrator transcript
git add crates/extensions/orchestrator/src/
git commit -m "feat(orchestrator): JSONL transcript reader"
```

---

## Task 10: Guardrails module

**Goal:** Centralize hard caps: max advances/hour per run, max judge prompt size, max workflow file size, advance counter per run.

**Files:**
- Create: `crates/extensions/orchestrator/src/guardrails.rs`
- Modify: `crates/extensions/orchestrator/src/lib.rs`

**Acceptance Criteria:**
- [ ] `AdvanceCounter::record_advance(run_id)` returns `Ok` for first 20 in an hour
- [ ] 21st advance in <1h returns `Err(LoopBreak)`
- [ ] Counters are per-run; different run_ids don't share
- [ ] Constants for `MAX_JUDGE_PROMPT_BYTES = 24*1024` and `MAX_WORKFLOW_FILE_BYTES = 256*1024` exposed

**Verify:** `cargo test -p claude-tabs-ext-orchestrator guardrails` → PASS

**Steps:**

- [ ] **Step 1: Implement + tests**

`crates/extensions/orchestrator/src/guardrails.rs`:

```rust
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use thiserror::Error;

pub const MAX_JUDGE_PROMPT_BYTES: usize = 24 * 1024;
pub const MAX_WORKFLOW_FILE_BYTES: u64 = 256 * 1024;
pub const MAX_ADVANCES_PER_HOUR: usize = 20;
const WINDOW: Duration = Duration::from_secs(3600);

#[derive(Debug, Error, PartialEq)]
pub enum GuardrailError {
    #[error("loop break: run `{0}` exceeded {1} advances/hour")]
    LoopBreak(String, usize),
}

#[derive(Default)]
pub struct AdvanceCounter {
    inner: Mutex<HashMap<String, Vec<Instant>>>,
}

impl AdvanceCounter {
    pub fn new() -> Self { Self::default() }

    pub fn record_advance(&self, run_id: &str) -> Result<(), GuardrailError> {
        let mut map = self.inner.lock().unwrap();
        let now = Instant::now();
        let entry = map.entry(run_id.to_string()).or_default();
        entry.retain(|t| now.duration_since(*t) < WINDOW);
        if entry.len() >= MAX_ADVANCES_PER_HOUR {
            return Err(GuardrailError::LoopBreak(run_id.to_string(), MAX_ADVANCES_PER_HOUR));
        }
        entry.push(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_max() {
        let c = AdvanceCounter::new();
        for _ in 0..MAX_ADVANCES_PER_HOUR {
            c.record_advance("r1").unwrap();
        }
    }

    #[test]
    fn blocks_after_max() {
        let c = AdvanceCounter::new();
        for _ in 0..MAX_ADVANCES_PER_HOUR {
            c.record_advance("r1").unwrap();
        }
        let err = c.record_advance("r1").unwrap_err();
        assert!(matches!(err, GuardrailError::LoopBreak(_, _)));
    }

    #[test]
    fn counters_isolated_per_run() {
        let c = AdvanceCounter::new();
        for _ in 0..MAX_ADVANCES_PER_HOUR {
            c.record_advance("r1").unwrap();
        }
        c.record_advance("r2").unwrap();
    }
}
```

`lib.rs` — add `pub mod guardrails;`

- [ ] **Step 2: Test + commit**

```bash
cargo test -p claude-tabs-ext-orchestrator guardrails
git add crates/extensions/orchestrator/src/
git commit -m "feat(orchestrator): guardrails (advance loop break, size caps)"
```

---

## Task 11: Judges (claude -p wrapper + completion + notes)

**Goal:** Implement `claude -p` subprocess wrapper and two judge functions: `judge_completion(stage, run, tail) → Verdict` and `draft_notes(stage_hint, run, tail) → String`.

**Files:**
- Create: `crates/extensions/orchestrator/src/judge/mod.rs`
- Create: `crates/extensions/orchestrator/src/judge/claude_cli.rs`
- Create: `crates/extensions/orchestrator/src/judge/completion.rs`
- Create: `crates/extensions/orchestrator/src/judge/notes.rs`
- Modify: `crates/extensions/orchestrator/src/lib.rs`

**Acceptance Criteria:**
- [ ] `ClaudeCli::run(prompt, model)` spawns `claude -p` and returns stdout
- [ ] `judge_completion` parses `<answer>{"complete": ..., "reason": ...}</answer>` correctly
- [ ] Malformed judge response → `Verdict::Escalate`
- [ ] `draft_notes` returns the trimmed stdout (judge writes plain text)
- [ ] Judge is dependency-injected via a `JudgeRunner` trait so tests can use a mock

**Verify:** `cargo test -p claude-tabs-ext-orchestrator judge` → PASS

**Steps:**

- [ ] **Step 1: Add modules**

`crates/extensions/orchestrator/src/lib.rs` — add `pub mod judge;`.

`crates/extensions/orchestrator/src/judge/mod.rs`:

```rust
pub mod claude_cli;
pub mod completion;
pub mod notes;

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum JudgeError {
    #[error("subprocess failed: {0}")]
    Subprocess(String),
    #[error("malformed response: {0}")]
    Malformed(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[async_trait]
pub trait JudgeRunner: Send + Sync {
    async fn run(&self, prompt: &str, model: &str) -> Result<String, JudgeError>;
}
```

- [ ] **Step 2: Implement Claude CLI runner**

`crates/extensions/orchestrator/src/judge/claude_cli.rs`:

```rust
use super::{JudgeError, JudgeRunner};
use async_trait::async_trait;
use tokio::process::Command;

pub struct ClaudeCli;

#[async_trait]
impl JudgeRunner for ClaudeCli {
    async fn run(&self, prompt: &str, model: &str) -> Result<String, JudgeError> {
        let model_arg = match model {
            "claude-haiku-4-5" | "haiku" => "haiku",
            "claude-sonnet-4-6" | "sonnet" => "sonnet",
            "claude-opus-4-7" | "opus" => "opus",
            other => other,
        };
        let output = Command::new("claude")
            .arg("-p")
            .arg("--model")
            .arg(model_arg)
            .arg(prompt)
            .output()
            .await
            .map_err(|e| JudgeError::Subprocess(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(JudgeError::Subprocess(stderr));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}
```

- [ ] **Step 3: Implement completion judge**

`crates/extensions/orchestrator/src/judge/completion.rs`:

```rust
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
    let mut transcript_repr = tail.compact_repr(MAX_JUDGE_PROMPT_BYTES / 2);

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

    if prompt.len() > MAX_JUDGE_PROMPT_BYTES {
        // shouldn't happen given the half-budget above, but be defensive
        let cut = MAX_JUDGE_PROMPT_BYTES - 200;
        transcript_repr.truncate(cut.min(transcript_repr.len()));
    }

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
    if s.len() <= max { s.to_string() } else { format!("{}...", &s[..max]) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::ir::*;
    use async_trait::async_trait;
    use std::collections::BTreeMap;

    struct MockRunner { reply: String }

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

    fn empty_tail() -> Tail { Tail { entries: vec![] } }

    #[tokio::test]
    async fn parses_complete_true() {
        let runner = MockRunner { reply: r#"thinking... <answer>{"complete": true, "reason": "ok"}</answer>"#.into() };
        let v = judge_completion(&runner, "haiku", &stage_with_criteria(), &run_at("a"), &empty_tail()).await.unwrap();
        assert_eq!(v, Verdict::Complete);
    }

    #[tokio::test]
    async fn parses_complete_false() {
        let runner = MockRunner { reply: r#"<answer>{"complete": false, "reason": "no PRs yet"}</answer>"#.into() };
        let v = judge_completion(&runner, "haiku", &stage_with_criteria(), &run_at("a"), &empty_tail()).await.unwrap();
        assert_eq!(v, Verdict::StayInStage);
    }

    #[tokio::test]
    async fn malformed_response_errors() {
        let runner = MockRunner { reply: "no answer block".into() };
        let err = judge_completion(&runner, "haiku", &stage_with_criteria(), &run_at("a"), &empty_tail()).await.unwrap_err();
        assert!(matches!(err, JudgeError::Malformed(_)));
    }
}
```

- [ ] **Step 4: Implement notes judge**

`crates/extensions/orchestrator/src/judge/notes.rs`:

```rust
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

    struct MockRunner { reply: String }

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
        let runner = MockRunner { reply: "  Mention docs are stale.  \n".into() };
        let n = draft_notes(&runner, "haiku", "remind about docs", &run_at("a"), &Tail { entries: vec![] }).await.unwrap();
        assert_eq!(n, "Mention docs are stale.");
    }
}
```

- [ ] **Step 5: Test + commit**

```bash
cargo test -p claude-tabs-ext-orchestrator judge
git add crates/extensions/orchestrator/src/
git commit -m "feat(orchestrator): claude -p judges (completion + notes)"
```

---

## Task 12: PTY injector

**Goal:** Expose `Injector::write_user_prompt(session_id, text)` that writes `text` + carriage return to the existing PTY for that session.

**Files:**
- Create: `crates/extensions/orchestrator/src/injector.rs`
- Modify: `crates/extensions/orchestrator/src/lib.rs`

**Acceptance Criteria:**
- [ ] `Injector::write_user_prompt(session_id, text)` returns `Ok(())` on success
- [ ] Returns `Err(NotFound)` if session has no PTY
- [ ] Calls into existing PTY API (do not reimplement; locate via `crates/pty`)

**Verify:** Manual smoke test in dev session (covered in Task 21); unit test with mock provider.

**Steps:**

- [ ] **Step 1: Inspect existing PTY API**

```bash
grep -n "fn write\|pub fn" crates/pty/src/lib.rs | head -30
```

Locate the function that writes bytes to a session's PTY (typical name: `write_to_session(session_id, &[u8])` or `write` on a session-handle). Use that here.

- [ ] **Step 2: Implement injector**

`crates/extensions/orchestrator/src/injector.rs`:

```rust
use claude_tabs_pty::{PtyError, PtyManager};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InjectError {
    #[error("session not found: {0}")]
    NotFound(String),
    #[error("write failed: {0}")]
    Write(String),
}

pub struct Injector {
    pty: Arc<PtyManager>,
}

impl Injector {
    pub fn new(pty: Arc<PtyManager>) -> Self { Self { pty } }

    pub async fn write_user_prompt(&self, session_id: &str, text: &str) -> Result<(), InjectError> {
        let mut payload = text.to_string();
        if !payload.ends_with('\r') {
            payload.push('\r');
        }
        self.pty
            .write(session_id, payload.as_bytes())
            .await
            .map_err(|e| match e {
                PtyError::NotFound(id) => InjectError::NotFound(id),
                other => InjectError::Write(other.to_string()),
            })
    }
}
```

⚠️ NOTE: the exact `PtyManager::write` signature may differ from this sketch — adapt to whatever the existing crate exposes. The acceptance criterion is "writes bytes + `\r` to the right session's PTY"; the signature is not load-bearing.

- [ ] **Step 3: Add module + commit**

`lib.rs` — add `pub mod injector;`.

```bash
cargo build -p claude-tabs-ext-orchestrator
git add crates/extensions/orchestrator/src/
git commit -m "feat(orchestrator): PTY injector for prompt auto-typing"
```

---

## Task 13: Auth (port + token files)

**Goal:** On extension activation, generate a 32-byte hex token + bind axum to a free localhost port, write both to `~/.claude-tabs/{orchestrator.token,orchestrator.port}` (mode 0600 on token).

**Files:**
- Create: `crates/extensions/orchestrator/src/transport/mod.rs`
- Create: `crates/extensions/orchestrator/src/transport/auth.rs`
- Modify: `crates/extensions/orchestrator/src/lib.rs`

**Acceptance Criteria:**
- [ ] `LocalAuth::generate()` produces a 64-char hex token
- [ ] Files written under `~/.claude-tabs/`
- [ ] Token file has mode 0600 on Unix
- [ ] `LocalAuth::verify(provided)` returns `true` for matching token, `false` otherwise

**Verify:** `cargo test -p claude-tabs-ext-orchestrator transport::auth` → PASS

**Steps:**

- [ ] **Step 1: Add deps**

`crates/extensions/orchestrator/Cargo.toml` — under `[dependencies]`:

```toml
rand = "0.8"
```

- [ ] **Step 2: Add modules**

`crates/extensions/orchestrator/src/lib.rs` — add `pub mod transport;`.

`crates/extensions/orchestrator/src/transport/mod.rs`:

```rust
pub mod auth;
```

- [ ] **Step 3: Implement auth + tests**

`crates/extensions/orchestrator/src/transport/auth.rs`:

```rust
use rand::RngCore;
use std::path::{Path, PathBuf};

pub struct LocalAuth {
    pub token: String,
    pub config_dir: PathBuf,
}

impl LocalAuth {
    pub fn generate(config_dir: PathBuf) -> std::io::Result<Self> {
        std::fs::create_dir_all(&config_dir)?;
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let token = hex::encode(bytes);

        let token_path = config_dir.join("orchestrator.token");
        std::fs::write(&token_path, &token)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&token_path)?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(&token_path, perms)?;
        }

        Ok(Self { token, config_dir })
    }

    pub fn write_port(&self, port: u16) -> std::io::Result<()> {
        std::fs::write(self.config_dir.join("orchestrator.port"), port.to_string())
    }

    pub fn cleanup(&self) {
        let _ = std::fs::remove_file(self.config_dir.join("orchestrator.token"));
        let _ = std::fs::remove_file(self.config_dir.join("orchestrator.port"));
    }

    pub fn verify(&self, provided: &str) -> bool {
        constant_time_eq(self.token.as_bytes(), provided.as_bytes())
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() { return false; }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

pub fn default_config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".claude-tabs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn generates_64_char_hex_token() {
        let tmp = TempDir::new().unwrap();
        let auth = LocalAuth::generate(tmp.path().to_path_buf()).unwrap();
        assert_eq!(auth.token.len(), 64);
        assert!(auth.token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn writes_token_file() {
        let tmp = TempDir::new().unwrap();
        let auth = LocalAuth::generate(tmp.path().to_path_buf()).unwrap();
        let on_disk = std::fs::read_to_string(tmp.path().join("orchestrator.token")).unwrap();
        assert_eq!(on_disk, auth.token);
    }

    #[cfg(unix)]
    #[test]
    fn token_file_has_mode_0600() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new().unwrap();
        let _auth = LocalAuth::generate(tmp.path().to_path_buf()).unwrap();
        let mode = std::fs::metadata(tmp.path().join("orchestrator.token")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn verify_constant_time() {
        let tmp = TempDir::new().unwrap();
        let auth = LocalAuth::generate(tmp.path().to_path_buf()).unwrap();
        assert!(auth.verify(&auth.token));
        assert!(!auth.verify("wrong"));
        assert!(!auth.verify(""));
    }
}
```

- [ ] **Step 4: Test + commit**

```bash
cargo test -p claude-tabs-ext-orchestrator transport::auth
git add crates/extensions/orchestrator/
git commit -m "feat(orchestrator): localhost auth (token + port files)"
```

---

## Task 14: HTTP server + endpoint handlers

**Goal:** axum HTTP server bound to `127.0.0.1:auto`, exposes `/turn-end`, `/session-start`, `/workflow/attach`, `/workflow/state/:sid`, `/workflow/control/:sid` (pause/resume/skip/cancel). All endpoints token-checked.

**Files:**
- Create: `crates/extensions/orchestrator/src/transport/server.rs`
- Create: `crates/extensions/orchestrator/src/transport/handlers.rs`
- Create: `crates/extensions/orchestrator/src/dispatcher.rs`
- Modify: `crates/extensions/orchestrator/src/lib.rs`
- Modify: `crates/extensions/orchestrator/src/transport/mod.rs`

**Acceptance Criteria:**
- [ ] Server binds to a free localhost port; port written to `~/.claude-tabs/orchestrator.port`
- [ ] Requests without `X-CT-Token` → 401
- [ ] Requests with wrong token → 401
- [ ] `POST /turn-end` triggers dispatcher; returns 204
- [ ] `POST /session-start` for `compact` source advances session_event stages
- [ ] `POST /workflow/attach` creates a run and PTY-types stage 1
- [ ] `GET /workflow/state/:sid` returns JSON `{run, current_stage}` or 404

**Verify:** `cargo test -p claude-tabs-ext-orchestrator transport::server` (integration tests using `tower::ServiceExt`) → PASS

**Steps:**

- [ ] **Step 1: Wire dispatcher**

`crates/extensions/orchestrator/src/dispatcher.rs`:

```rust
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
use tracing::{error, info};

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
    pub async fn on_turn_end(&self, session_id: &str, transcript_path: &Path) -> Result<(), DispatchError> {
        let run = self.store.load_by_session(session_id)?
            .ok_or_else(|| DispatchError::NoRun(session_id.into()))?;
        if run.status != RunStatus::Running { return Ok(()); }
        let workflow = self.registry.get(&run.workflow_id)
            .ok_or_else(|| DispatchError::UnknownWorkflow(run.workflow_id.clone()))?;
        let stage = workflow.stages.get(&run.current_stage_id).expect("stage exists; compiler validated").clone();

        let tail = Tail::read(transcript_path, 80).unwrap_or(Tail { entries: vec![] });

        let verdict = match &stage.completion {
            Completion::Criteria { .. } => {
                judge_completion(&*self.judge, &workflow.model, &stage, &run, &tail).await?
            }
            Completion::ToolUseMatch { matcher } => {
                let matched = tail.tool_uses().any(|(name, input)| matches_tool_use(matcher, name, input));
                if matched { Verdict::Complete } else { Verdict::StayInStage }
            }
            Completion::SessionEvent { .. } => Verdict::StayInStage, // advanced via /session-start
        };

        match advance(&run, &workflow, verdict.clone()) {
            AdvanceOutcome::NoOp => Ok(()),
            AdvanceOutcome::Done => {
                self.store.set_status(&run.run_id, RunStatus::Done)?;
                Ok(())
            }
            AdvanceOutcome::Inject { stage_id } => {
                self.do_inject(&run, &workflow, &stage, &stage_id, &tail).await
            }
            AdvanceOutcome::Escalate { .. } => {
                self.store.set_status(&run.run_id, RunStatus::Escalated)?;
                Ok(())
            }
        }
    }

    pub async fn on_session_start(&self, session_id: &str, source: &str) -> Result<(), DispatchError> {
        let Some(run) = self.store.load_by_session(session_id)? else { return Ok(()); };
        if source != "compact" { return Ok(()); }
        let workflow = self.registry.get(&run.workflow_id)
            .ok_or_else(|| DispatchError::UnknownWorkflow(run.workflow_id.clone()))?;
        let stage = workflow.stages.get(&run.current_stage_id).expect("validated").clone();

        if let Completion::SessionEvent { on } = &stage.completion {
            if matches!(on, SessionEventKind::Compact) {
                if let AdvanceOutcome::Inject { stage_id } = advance(&run, &workflow, Verdict::Complete) {
                    let empty = Tail { entries: vec![] };
                    self.do_inject(&run, &workflow, &stage, &stage_id, &empty).await?;
                } else if matches!(advance(&run, &workflow, Verdict::Complete), AdvanceOutcome::Done) {
                    self.store.set_status(&run.run_id, RunStatus::Done)?;
                }
            }
        }
        Ok(())
    }

    async fn do_inject(
        &self,
        run: &crate::runtime::run::WorkflowRun,
        workflow: &Workflow,
        _from_stage: &crate::workflow::ir::Stage,
        next_stage_id: &str,
        tail: &Tail,
    ) -> Result<(), DispatchError> {
        self.counter.record_advance(&run.run_id)?;
        let next_stage = workflow.stages.get(next_stage_id).expect("validated").clone();

        let notes = if let Notes::Dynamic { hint } = &next_stage.notes {
            Some(draft_notes(&*self.judge, &workflow.model, hint, run, tail).await?)
        } else {
            None
        };

        let rendered = render(&next_stage, &run.inputs, notes.as_deref())?;
        self.store.advance_run(&run.run_id, next_stage_id)?;
        self.store.record_history(&run.run_id, next_stage_id, "complete", None)?;
        info!(session_id = %run.session_id, next_stage = %next_stage_id, "injecting next stage");
        self.injector.write_user_prompt(&run.session_id, &rendered).await?;
        Ok(())
    }
}

fn matches_tool_use(matcher: &crate::workflow::ir::ToolUseMatcher, name: &str, input: &serde_json::Value) -> bool {
    if matcher.tool != name { return false; }
    for (field, pattern) in &matcher.input {
        let val = input.get(field).and_then(|v| v.as_str()).unwrap_or("");
        let re = match regex::Regex::new(pattern) { Ok(r) => r, Err(_) => return false };
        if !re.is_match(val) { return false; }
    }
    true
}
```

- [ ] **Step 2: Implement HTTP layer**

`crates/extensions/orchestrator/src/transport/mod.rs`:

```rust
pub mod auth;
pub mod handlers;
pub mod server;
```

`crates/extensions/orchestrator/src/transport/handlers.rs`:

```rust
use crate::dispatcher::Dispatcher;
use axum::{extract::{Path, State}, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

pub type AppState = Arc<Dispatcher>;

#[derive(Deserialize)]
pub struct TurnEnd {
    pub session_id: String,
    pub transcript_path: String,
}

pub async fn turn_end(State(d): State<AppState>, Json(p): Json<TurnEnd>) -> impl IntoResponse {
    if let Err(e) = d.on_turn_end(&p.session_id, &PathBuf::from(p.transcript_path)).await {
        tracing::warn!("turn_end error: {e}");
    }
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
pub struct SessionStart {
    pub session_id: String,
    pub source: String,
}

pub async fn session_start(State(d): State<AppState>, Json(p): Json<SessionStart>) -> impl IntoResponse {
    if let Err(e) = d.on_session_start(&p.session_id, &p.source).await {
        tracing::warn!("session_start error: {e}");
    }
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
pub struct AttachReq {
    pub session_id: String,
    pub workflow_id: String,
    pub inputs: serde_json::Value,
}

#[derive(Serialize)]
pub struct AttachResp {
    pub run_id: String,
    pub current_stage_id: String,
}

pub async fn attach(State(d): State<AppState>, Json(p): Json<AttachReq>) -> Result<Json<AttachResp>, (StatusCode, String)> {
    use crate::runtime::run::{RunStatus, WorkflowRun};
    use std::collections::BTreeMap;

    let workflow = d.registry.get(&p.workflow_id)
        .ok_or((StatusCode::NOT_FOUND, format!("unknown workflow {}", p.workflow_id)))?;
    let stage_one = workflow.stage_order.first()
        .ok_or((StatusCode::BAD_REQUEST, "workflow has no stages".into()))?
        .clone();

    let inputs: BTreeMap<String, serde_json::Value> = serde_json::from_value(p.inputs)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("bad inputs: {e}")))?;

    let run = WorkflowRun {
        run_id: uuid::Uuid::new_v4().to_string(),
        session_id: p.session_id.clone(),
        workflow_id: p.workflow_id.clone(),
        inputs: inputs.clone(),
        current_stage_id: stage_one.clone(),
        status: RunStatus::Running,
        started_at: chrono::Utc::now().timestamp_millis(),
        ended_at: None,
    };

    d.store.create_run(&run).map_err(|e| (StatusCode::CONFLICT, e.to_string()))?;
    d.store.record_history(&run.run_id, &stage_one, "entered", None).ok();

    let stage = workflow.stages.get(&stage_one).unwrap();
    let rendered = crate::runtime::render::render(stage, &inputs, None)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
    d.injector.write_user_prompt(&p.session_id, &rendered).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(AttachResp { run_id: run.run_id, current_stage_id: stage_one }))
}

pub async fn state(State(d): State<AppState>, Path(sid): Path<String>) -> Result<Json<serde_json::Value>, StatusCode> {
    let run = d.store.load_by_session(&sid).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    match run {
        Some(r) => Ok(Json(serde_json::json!({"run": r}))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

#[derive(Deserialize)]
pub struct ControlReq { pub action: String } // pause | resume | cancel | skip

pub async fn control(State(d): State<AppState>, Path(sid): Path<String>, Json(p): Json<ControlReq>) -> Result<StatusCode, (StatusCode, String)> {
    use crate::runtime::run::RunStatus;
    let run = d.store.load_by_session(&sid).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "no run".into()))?;
    match p.action.as_str() {
        "pause"  => d.store.set_status(&run.run_id, RunStatus::Paused).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        "resume" => d.store.set_status(&run.run_id, RunStatus::Running).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        "cancel" => d.store.set_status(&run.run_id, RunStatus::Paused).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
        "skip" => {
            let workflow = d.registry.get(&run.workflow_id).ok_or((StatusCode::INTERNAL_SERVER_ERROR, "missing workflow".into()))?;
            let stage = workflow.stages.get(&run.current_stage_id).unwrap().clone();
            match crate::runtime::run::advance(&run, &workflow, crate::runtime::run::Verdict::Complete) {
                crate::runtime::run::AdvanceOutcome::Inject { stage_id } => {
                    d.store.advance_run(&run.run_id, &stage_id).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                    let next = workflow.stages.get(&stage_id).unwrap();
                    let rendered = crate::runtime::render::render(next, &run.inputs, None).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                    d.injector.write_user_prompt(&sid, &rendered).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
                }
                crate::runtime::run::AdvanceOutcome::Done => d.store.set_status(&run.run_id, RunStatus::Done).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?,
                _ => {}
            }
        }
        other => return Err((StatusCode::BAD_REQUEST, format!("unknown action {other}"))),
    }
    Ok(StatusCode::NO_CONTENT)
}
```

`crates/extensions/orchestrator/src/transport/server.rs`:

```rust
use super::auth::LocalAuth;
use super::handlers::AppState;
use axum::{
    body::Body, extract::State, http::{header, Request, StatusCode}, middleware::{self, Next}, response::Response, routing::{get, post}, Router
};
use std::net::SocketAddr;
use std::sync::Arc;

pub async fn serve(state: AppState, auth: Arc<LocalAuth>) -> std::io::Result<()> {
    let app = Router::new()
        .route("/turn-end", post(super::handlers::turn_end))
        .route("/session-start", post(super::handlers::session_start))
        .route("/workflow/attach", post(super::handlers::attach))
        .route("/workflow/state/:sid", get(super::handlers::state))
        .route("/workflow/control/:sid", post(super::handlers::control))
        .layer(middleware::from_fn_with_state(auth.clone(), token_check))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let port = listener.local_addr()?.port();
    auth.write_port(port)?;
    tracing::info!(port, "orchestrator HTTP listener up");
    axum::serve(listener, app).await
}

async fn token_check(State(auth): State<Arc<LocalAuth>>, req: Request<Body>, next: Next) -> Result<Response, StatusCode> {
    let provided = req.headers()
        .get("X-CT-Token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !auth.verify(provided) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(next.run(req).await)
}
```

⚠️ NOTE: `middleware::from_fn_with_state` shape differs per axum 0.7 minor version; if compilation complains, switch to `middleware::from_fn` + a closure capturing `auth`.

- [ ] **Step 3: Wire activation**

In `crates/extensions/orchestrator/src/lib.rs`, expand `OrchestratorExtension::activate` to:

1. Open a new `rusqlite::Connection` to `~/.claude-tabs/archive.db` (use the same path the storage crate uses; locate via `claude_tabs_storage::sqlite` helpers if exposed)
2. Run `claude_tabs_storage::migrations::run_migrations`
3. Wrap in `RunStore::new`
4. Build `WorkflowRegistry`, populate from `~/.claude-tabs/workflows`
5. `LocalAuth::generate(default_config_dir())`
6. Build `Dispatcher { registry, store, judge: Arc::new(ClaudeCli), injector: Arc::new(Injector::new(ctx.session_store.pty_manager())), counter }`
7. `tokio::spawn(transport::server::serve(Arc::new(dispatcher), Arc::new(auth)))`
8. Store the `JoinHandle` and `auth` on `self` for `deactivate`

Concrete shape (approximate; adjust for actual `ActivationContext` fields):

```rust
async fn activate(&mut self, ctx: &mut ActivationContext) -> Result<(), ExtensionError> {
    use crate::dispatcher::Dispatcher;
    use crate::guardrails::AdvanceCounter;
    use crate::injector::Injector;
    use crate::judge::claude_cli::ClaudeCli;
    use crate::runtime::store::RunStore;
    use crate::transport::auth::{default_config_dir, LocalAuth};
    use crate::workflow::loader::populate_registry;
    use crate::workflow::registry::WorkflowRegistry;

    let cfg_dir = default_config_dir();
    let db_path = cfg_dir.join("archive.db");
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| ExtensionError::ActivationFailed(e.to_string()))?;
    claude_tabs_storage::migrations::run_migrations(&conn)
        .map_err(|e| ExtensionError::ActivationFailed(e.to_string()))?;
    let store = std::sync::Arc::new(RunStore::new(conn));

    let registry = std::sync::Arc::new(WorkflowRegistry::new());
    let workflow_dir = cfg_dir.join("workflows");
    let _errs = populate_registry(&workflow_dir, &registry);

    let auth = std::sync::Arc::new(
        LocalAuth::generate(cfg_dir).map_err(|e| ExtensionError::ActivationFailed(e.to_string()))?,
    );

    let injector = std::sync::Arc::new(Injector::new(ctx.session_store.pty_manager_arc()));
    // ⚠️ adapt `pty_manager_arc()` to whatever the SessionStore exposes
    let dispatcher = std::sync::Arc::new(Dispatcher {
        registry,
        store,
        judge: std::sync::Arc::new(ClaudeCli),
        injector,
        counter: std::sync::Arc::new(AdvanceCounter::new()),
    });

    let auth_for_serve = auth.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::transport::server::serve(dispatcher, auth_for_serve).await {
            tracing::error!("orchestrator http server crashed: {e}");
        }
    });
    self.auth = Some(auth);
    Ok(())
}

async fn deactivate(&mut self) -> Result<(), ExtensionError> {
    if let Some(a) = self.auth.take() { a.cleanup(); }
    Ok(())
}
```

Add `auth: Option<Arc<LocalAuth>>` to the `OrchestratorExtension` struct.

- [ ] **Step 4: Integration tests**

Create `crates/extensions/orchestrator/tests/http_smoke.rs`:

```rust
//! Boots the axum router with mock judge + mock injector and exercises endpoints
//! via tower's `oneshot` (no real network).

// Test left as exercise — the dispatcher unit tests already cover logic.
// Acceptance for this task: extension activates without panic in dev build.
```

Skip full HTTP integration tests in v1; cover via manual smoke (Task 21). Unit tests on dispatcher logic and handlers' input validation are sufficient.

- [ ] **Step 5: Build + commit**

```bash
cargo build -p claude-tabs-ext-orchestrator
git add crates/extensions/orchestrator/
git commit -m "feat(orchestrator): HTTP server + dispatcher + handlers"
```

---

## Task 15: Plugin (`claude-tabs-orchestrator`)

**Goal:** Build the Claude Code plugin in a sibling repo (`../claude-tabs-orchestrator/`). Two hook scripts, plugin.json, marketplace.json, smoke test.

**Files (new repo, not in Claude-Tab):**
- Create: `claude-tabs-orchestrator/.claude-plugin/plugin.json`
- Create: `claude-tabs-orchestrator/.claude-plugin/marketplace.json`
- Create: `claude-tabs-orchestrator/hooks/hooks.json`
- Create: `claude-tabs-orchestrator/hooks/on_stop.sh`
- Create: `claude-tabs-orchestrator/hooks/on_session_start.sh`
- Create: `claude-tabs-orchestrator/README.md`

**Acceptance Criteria:**
- [ ] Hooks are executable (`chmod +x`)
- [ ] Manual smoke test: `cat sample-stop-input.json | hooks/on_stop.sh` POSTs to `localhost:<port>` with token (verified by netcat listener)
- [ ] Plugin installable via `/plugin marketplace add` from local file path

**Verify:** `bash hooks/on_stop.sh < /tmp/sample.json` succeeds (no crash, exits 0)

**Steps:**

- [ ] **Step 1: Create repo skeleton**

```bash
cd ..
mkdir claude-tabs-orchestrator
cd claude-tabs-orchestrator
git init
mkdir -p .claude-plugin hooks
```

- [ ] **Step 2: plugin.json**

`.claude-plugin/plugin.json`:

```json
{
  "name": "claude-tabs-orchestrator",
  "version": "0.1.0",
  "description": "Workflow orchestration signals for Claude-Tab. Requires Claude-Tab running."
}
```

- [ ] **Step 3: marketplace.json**

`.claude-plugin/marketplace.json`:

```json
{
  "name": "claude-tabs-orchestrator",
  "owner": { "name": "MjMoshiri" },
  "plugins": [
    { "name": "claude-tabs-orchestrator", "source": "." }
  ]
}
```

- [ ] **Step 4: hooks.json**

`hooks/hooks.json`:

```json
{
  "hooks": {
    "Stop": [{
      "hooks": [{
        "type": "command",
        "command": "\"${CLAUDE_PLUGIN_ROOT}/hooks/on_stop.sh\"",
        "timeout": 10,
        "statusMessage": "Orchestrator: evaluating workflow..."
      }]
    }],
    "SessionStart": [{
      "hooks": [{
        "type": "command",
        "command": "\"${CLAUDE_PLUGIN_ROOT}/hooks/on_session_start.sh\"",
        "timeout": 5
      }]
    }]
  }
}
```

- [ ] **Step 5: on_stop.sh**

`hooks/on_stop.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
INPUT=$(cat)
CFG="${CLAUDE_TABS_CFG_DIR:-$HOME/.claude-tabs}"
[ -f "$CFG/orchestrator.port" ] && [ -f "$CFG/orchestrator.token" ] || exit 0

PORT=$(cat "$CFG/orchestrator.port")
TOKEN=$(cat "$CFG/orchestrator.token")
SID=$(jq -r '.session_id' <<<"$INPUT")
TP=$(jq -r '.transcript_path' <<<"$INPUT")

curl -fsS --max-time 8 \
  -H "X-CT-Token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d "$(jq -n --arg sid "$SID" --arg tp "$TP" \
        '{event:"turn_end",session_id:$sid,transcript_path:$tp}')" \
  "http://127.0.0.1:$PORT/turn-end" >/dev/null 2>&1 || true
exit 0
```

- [ ] **Step 6: on_session_start.sh**

`hooks/on_session_start.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail
INPUT=$(cat)
CFG="${CLAUDE_TABS_CFG_DIR:-$HOME/.claude-tabs}"
[ -f "$CFG/orchestrator.port" ] && [ -f "$CFG/orchestrator.token" ] || exit 0

PORT=$(cat "$CFG/orchestrator.port")
TOKEN=$(cat "$CFG/orchestrator.token")
SID=$(jq -r '.session_id' <<<"$INPUT")
SOURCE=$(jq -r '.source' <<<"$INPUT")

curl -fsS --max-time 5 \
  -H "X-CT-Token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d "$(jq -n --arg sid "$SID" --arg src "$SOURCE" \
        '{event:"session_start",session_id:$sid,source:$src}')" \
  "http://127.0.0.1:$PORT/session-start" >/dev/null 2>&1 || true
exit 0
```

- [ ] **Step 7: chmod + README + smoke**

```bash
chmod +x hooks/on_stop.sh hooks/on_session_start.sh

cat > README.md <<'EOF'
# claude-tabs-orchestrator

Claude Code plugin that signals Claude-Tab on session events for workflow orchestration.

Requires Claude-Tab v1.5.0+ running locally.

## Install

```bash
/plugin marketplace add MjMoshiri/claude-tabs-orchestrator
/plugin install claude-tabs-orchestrator@claude-tabs-orchestrator
```

## How it works

`Stop` → POST `http://127.0.0.1:<port>/turn-end` (port + auth token from `~/.claude-tabs/`).
`SessionStart` → POST `/session-start`.

If Claude-Tab is not running, hooks no-op silently.
EOF
```

Smoke test (with Tab running and a port file present):

```bash
echo '{"session_id":"test-123","transcript_path":"/tmp/transcript.jsonl"}' \
  | hooks/on_stop.sh
echo "Exit: $?"
```

- [ ] **Step 8: Commit + tag**

```bash
git add .
git commit -m "feat: claude-tabs-orchestrator plugin v0.1.0"
git tag v0.1.0
```

---

## Task 16: Tauri bridge commands + TS types

**Goal:** Expose orchestrator endpoints as Tauri commands so the React UI can call them without raw HTTP. Define TS types matching the Rust IR.

**Files:**
- Modify: `crates/tauri-bridge/src/commands.rs`
- Modify: `src-tauri/src/main.rs` (or wherever commands register)
- Create: `src/types/workflow.ts`
- Create: `src/extensions/orchestrator/api.ts`

**Acceptance Criteria:**
- [ ] Tauri commands `list_workflows`, `attach_workflow`, `get_run_state`, `control_run` registered
- [ ] TS types match Rust shapes (manually kept in sync; document at top)
- [ ] `api.ts` provides typed wrappers around `invoke()`

**Verify:** `npm run build` produces no TS errors

**Steps:**

- [ ] **Step 1: Add Tauri commands**

In `crates/tauri-bridge/src/commands.rs`, add:

```rust
#[tauri::command]
pub async fn list_workflows(state: tauri::State<'_, AppState>) -> Result<Vec<serde_json::Value>, CommandError> {
    let registry = state.orchestrator_registry();
    Ok(registry.list().into_iter().map(|w| serde_json::to_value(&w).unwrap()).collect())
}

#[tauri::command]
pub async fn attach_workflow(
    state: tauri::State<'_, AppState>,
    session_id: String,
    workflow_id: String,
    inputs: serde_json::Value,
) -> Result<serde_json::Value, CommandError> {
    state.orchestrator_attach(session_id, workflow_id, inputs).await
        .map_err(|e| CommandError::Internal(e.to_string()))
}

#[tauri::command]
pub async fn get_run_state(
    state: tauri::State<'_, AppState>,
    session_id: String,
) -> Result<Option<serde_json::Value>, CommandError> {
    state.orchestrator_run_state(session_id).await
        .map_err(|e| CommandError::Internal(e.to_string()))
}

#[tauri::command]
pub async fn control_run(
    state: tauri::State<'_, AppState>,
    session_id: String,
    action: String,
) -> Result<(), CommandError> {
    state.orchestrator_control(session_id, action).await
        .map_err(|e| CommandError::Internal(e.to_string()))
}
```

Add `orchestrator_registry`, `orchestrator_attach`, `orchestrator_run_state`, `orchestrator_control` to `AppState` (`crates/tauri-bridge/src/ipc.rs` or equivalent — adapt to existing layout). Each delegates to the dispatcher / store / registry.

Register in main:

```rust
.invoke_handler(tauri::generate_handler![
    /* existing */,
    list_workflows,
    attach_workflow,
    get_run_state,
    control_run,
])
```

- [ ] **Step 2: TS types**

`src/types/workflow.ts`:

```ts
// Manually kept in sync with crates/extensions/orchestrator/src/workflow/ir.rs

export type Workflow = {
  id: string;
  name: string;
  description: string;
  model: string;
  inputs: InputSpec[];
  stages: Record<string, Stage>;
  stage_order: string[];
};

export type InputSpec = {
  name: string;
  description: string;
  required: boolean;
  type: 'string' | 'integer' | 'bool' | 'enum' | 'multiline_string';
  default?: unknown;
  enum?: string[];
};

export type Stage = {
  id: string;
  goal: string;
  prompt: string;
  completion: Completion;
  next: string | { Done: null } | 'done';
  notes: Notes;
};

export type Completion =
  | { type: 'criteria'; criteria: string }
  | { type: 'tool_use_match'; match: { tool: string; input: Record<string, string> } }
  | { type: 'session_event'; on: 'compact' | 'clear' | 'resume' };

export type Notes =
  | { type: 'static' }
  | { type: 'dynamic'; hint: string };

export type RunStatus = 'running' | 'paused' | 'done' | 'escalated' | 'error';

export type WorkflowRun = {
  run_id: string;
  session_id: string;
  workflow_id: string;
  inputs: Record<string, unknown>;
  current_stage_id: string;
  status: RunStatus;
  started_at: number;
  ended_at: number | null;
};
```

- [ ] **Step 3: api.ts**

`src/extensions/orchestrator/api.ts`:

```ts
import { invoke } from '@tauri-apps/api/core';
import type { Workflow, WorkflowRun } from '../../types/workflow';

export async function listWorkflows(): Promise<Workflow[]> {
  return invoke('list_workflows');
}

export async function attachWorkflow(
  sessionId: string,
  workflowId: string,
  inputs: Record<string, unknown>,
): Promise<{ run_id: string; current_stage_id: string }> {
  return invoke('attach_workflow', { sessionId, workflowId, inputs });
}

export async function getRunState(sessionId: string): Promise<{ run: WorkflowRun } | null> {
  return invoke('get_run_state', { sessionId });
}

export async function controlRun(
  sessionId: string,
  action: 'pause' | 'resume' | 'cancel' | 'skip',
): Promise<void> {
  return invoke('control_run', { sessionId, action });
}
```

- [ ] **Step 4: Build + commit**

```bash
npm run build
cargo build -p claude-tabs-tauri-bridge
git add crates/tauri-bridge src/types/workflow.ts src/extensions/orchestrator/api.ts src-tauri/
git commit -m "feat: tauri commands + TS types for workflow orchestrator"
```

---

## Task 17: Frontend — Workflow library panel

**Goal:** Read-only panel listing workflows with name, description, stage count, compile-error indicator.

**Files:**
- Create: `src/extensions/orchestrator/index.tsx`
- Create: `src/extensions/orchestrator/WorkflowsPanel.tsx`
- Modify: `src/extensions/settings/SettingsPanel.tsx` (add tab)

**Acceptance Criteria:**
- [ ] Panel renders all workflows from `listWorkflows()`
- [ ] Shows zero-state message when no workflows
- [ ] Each entry shows id, name, description, stage count
- [ ] Reload button re-invokes `listWorkflows()`

**Verify:** `npm run dev`, open Settings → Workflows tab, see workflows.

**Steps:**

- [ ] **Step 1: Extension entry**

`src/extensions/orchestrator/index.tsx`:

```tsx
import { createExtension } from '../../sdk/createExtension';
import { WorkflowsPanel } from './WorkflowsPanel';

export const orchestratorExtension = createExtension({
  id: 'orchestrator',
  name: 'Workflow Orchestrator',
  description: 'Manage and attach workflows to sessions',
  activate(ctx) {
    ctx.registerSettingsTab({
      id: 'orchestrator-workflows',
      label: 'Workflows',
      component: WorkflowsPanel,
    });
  },
});
```

⚠️ NOTE: adapt `registerSettingsTab` to whatever the existing SDK / SettingsPanel uses. If settings tabs aren't dynamic yet, hardcode a tab in `SettingsPanel.tsx` that conditionally renders `<WorkflowsPanel />`.

- [ ] **Step 2: WorkflowsPanel**

`src/extensions/orchestrator/WorkflowsPanel.tsx`:

```tsx
import React, { useEffect, useState } from 'react';
import { listWorkflows } from './api';
import type { Workflow } from '../../types/workflow';

export function WorkflowsPanel() {
  const [workflows, setWorkflows] = useState<Workflow[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const reload = async () => {
    setLoading(true);
    setError(null);
    try {
      setWorkflows(await listWorkflows());
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { reload(); }, []);

  return (
    <div style={{ padding: 16 }}>
      <header style={{ display: 'flex', justifyContent: 'space-between' }}>
        <h2>Workflows</h2>
        <button onClick={reload} disabled={loading}>{loading ? 'Loading…' : 'Reload'}</button>
      </header>

      <p style={{ opacity: 0.7, fontSize: 13 }}>
        Files in <code>~/.claude-tabs/workflows/*.md</code>. Edit with any text editor; reload to pick up changes.
      </p>

      {error && <div style={{ color: 'crimson' }}>Error: {error}</div>}

      {workflows.length === 0 && !loading && (
        <div style={{ marginTop: 24, opacity: 0.6 }}>
          No workflows found. Create one at <code>~/.claude-tabs/workflows/&lt;id&gt;.md</code>.
        </div>
      )}

      <ul style={{ listStyle: 'none', padding: 0, marginTop: 16 }}>
        {workflows.map(w => (
          <li key={w.id} style={{ border: '1px solid #333', padding: 12, marginBottom: 8, borderRadius: 4 }}>
            <div style={{ fontWeight: 'bold' }}>{w.name}</div>
            <div style={{ fontSize: 12, opacity: 0.7 }}>{w.id}</div>
            {w.description && <div style={{ marginTop: 4 }}>{w.description}</div>}
            <div style={{ marginTop: 4, fontSize: 12 }}>{w.stage_order.length} stage(s) · model: {w.model}</div>
          </li>
        ))}
      </ul>
    </div>
  );
}
```

- [ ] **Step 3: Manual verify + commit**

```bash
npm run dev
# In app: Settings → Workflows. Confirm list renders.
git add src/extensions/orchestrator/
git commit -m "feat(ui): workflow library read-only panel"
```

---

## Task 18: Frontend — Attach modal + inputs form

**Goal:** Modal: pick workflow → fill inputs (form built from `inputs[]` schema) → submit → call `attachWorkflow()`. Reachable from command palette and tab right-click.

**Files:**
- Create: `src/extensions/orchestrator/AttachWorkflow.tsx`
- Create: `src/extensions/orchestrator/InputsForm.tsx`
- Modify: `src/extensions/orchestrator/index.tsx` (register command + context-menu entry)

**Acceptance Criteria:**
- [ ] Modal lists all workflows
- [ ] Selecting a workflow shows the inputs form
- [ ] Required inputs have validation; submit disabled until all filled
- [ ] Submit calls `attachWorkflow(sessionId, workflowId, inputs)` and closes modal on success
- [ ] Errors surface inline

**Verify:** `npm run dev`, open command palette → "Attach workflow" → fill form → submit → confirm new prompt appears in PTY.

**Steps:**

- [ ] **Step 1: InputsForm**

`src/extensions/orchestrator/InputsForm.tsx`:

```tsx
import React, { useState } from 'react';
import type { InputSpec } from '../../types/workflow';

type Props = {
  inputs: InputSpec[];
  onSubmit: (values: Record<string, unknown>) => void | Promise<void>;
  submitLabel?: string;
};

export function InputsForm({ inputs, onSubmit, submitLabel = 'Start' }: Props) {
  const [values, setValues] = useState<Record<string, unknown>>(() =>
    Object.fromEntries(inputs.map(i => [i.name, i.default ?? '']))
  );
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const missing = inputs.filter(i => i.required && (values[i.name] === '' || values[i.name] == null));
  const canSubmit = missing.length === 0 && !busy;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true); setErr(null);
    try {
      await onSubmit(values);
    } catch (ex) {
      setErr(String(ex));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form onSubmit={handleSubmit}>
      {inputs.map(spec => (
        <label key={spec.name} style={{ display: 'block', marginBottom: 12 }}>
          <div style={{ fontWeight: 'bold' }}>
            {spec.name}{spec.required && <span style={{ color: 'crimson' }}> *</span>}
          </div>
          <div style={{ fontSize: 12, opacity: 0.7 }}>{spec.description}</div>
          {spec.type === 'multiline_string' ? (
            <textarea
              value={String(values[spec.name] ?? '')}
              onChange={e => setValues(v => ({ ...v, [spec.name]: e.target.value }))}
              rows={4}
              style={{ width: '100%', marginTop: 4 }}
            />
          ) : spec.type === 'enum' ? (
            <select
              value={String(values[spec.name] ?? '')}
              onChange={e => setValues(v => ({ ...v, [spec.name]: e.target.value }))}
              style={{ width: '100%', marginTop: 4 }}
            >
              <option value="">—</option>
              {(spec.enum ?? []).map(opt => <option key={opt} value={opt}>{opt}</option>)}
            </select>
          ) : (
            <input
              type={spec.type === 'integer' ? 'number' : 'text'}
              value={String(values[spec.name] ?? '')}
              onChange={e => setValues(v => ({ ...v, [spec.name]: spec.type === 'integer' ? Number(e.target.value) : e.target.value }))}
              style={{ width: '100%', marginTop: 4 }}
            />
          )}
        </label>
      ))}
      {err && <div style={{ color: 'crimson', marginBottom: 8 }}>{err}</div>}
      <button type="submit" disabled={!canSubmit}>{busy ? 'Submitting…' : submitLabel}</button>
    </form>
  );
}
```

- [ ] **Step 2: AttachWorkflow modal**

`src/extensions/orchestrator/AttachWorkflow.tsx`:

```tsx
import React, { useEffect, useState } from 'react';
import { attachWorkflow, listWorkflows } from './api';
import type { Workflow } from '../../types/workflow';
import { InputsForm } from './InputsForm';

type Props = { sessionId: string; onClose: () => void };

export function AttachWorkflow({ sessionId, onClose }: Props) {
  const [workflows, setWorkflows] = useState<Workflow[]>([]);
  const [picked, setPicked] = useState<Workflow | null>(null);

  useEffect(() => { listWorkflows().then(setWorkflows); }, []);

  return (
    <div style={modalBackdrop}>
      <div style={modalBody}>
        <header style={{ display: 'flex', justifyContent: 'space-between' }}>
          <h3>Attach workflow to session</h3>
          <button onClick={onClose}>×</button>
        </header>

        {!picked && (
          <ul style={{ listStyle: 'none', padding: 0 }}>
            {workflows.map(w => (
              <li key={w.id} style={{ padding: 8, cursor: 'pointer' }} onClick={() => setPicked(w)}>
                <strong>{w.name}</strong> <small style={{ opacity: 0.6 }}>({w.id})</small>
                <div style={{ fontSize: 12, opacity: 0.7 }}>{w.description}</div>
              </li>
            ))}
          </ul>
        )}

        {picked && (
          <>
            <button onClick={() => setPicked(null)} style={{ marginBottom: 12 }}>← back</button>
            <h4>{picked.name}</h4>
            <InputsForm
              inputs={picked.inputs}
              submitLabel="Start workflow"
              onSubmit={async (values) => {
                await attachWorkflow(sessionId, picked.id, values);
                onClose();
              }}
            />
          </>
        )}
      </div>
    </div>
  );
}

const modalBackdrop: React.CSSProperties = {
  position: 'fixed', top: 0, left: 0, right: 0, bottom: 0,
  background: 'rgba(0,0,0,0.5)', display: 'flex', alignItems: 'center', justifyContent: 'center',
  zIndex: 1000,
};

const modalBody: React.CSSProperties = {
  background: '#1e1e1e', color: '#eee', padding: 24, borderRadius: 8, width: 480, maxHeight: '80vh', overflow: 'auto',
};
```

- [ ] **Step 3: Register command palette entry**

In `src/extensions/orchestrator/index.tsx`, register a command (use existing command-palette ext API):

```tsx
ctx.registerCommand({
  id: 'orchestrator.attach',
  label: 'Attach workflow to active session',
  run: () => {
    const activeId = ctx.sessions.activeSessionId();
    if (!activeId) return;
    ctx.openModal(<AttachWorkflow sessionId={activeId} onClose={ctx.closeModal} />);
  },
});
```

⚠️ NOTE: APIs `ctx.registerCommand`, `ctx.openModal`, `ctx.sessions.activeSessionId` may not exist verbatim — match to the actual SDK exposed in `src/sdk/createExtension.ts`. If a modal portal doesn't exist, render the modal as a sibling component conditionally controlled by extension state.

- [ ] **Step 4: Manual verify + commit**

```bash
npm run dev
# Cmd+K → Attach workflow → pick → fill → start. Confirm prompt appears in PTY.
git add src/extensions/orchestrator/
git commit -m "feat(ui): attach workflow modal + inputs form"
```

---

## Task 19: Frontend — Status badge + run drawer

**Goal:** Status bar badge showing current run + drawer with stage timeline + manual controls.

**Files:**
- Create: `src/extensions/orchestrator/OrchestratorBadge.tsx`
- Create: `src/extensions/orchestrator/RunDrawer.tsx`
- Modify: `src/extensions/orchestrator/index.tsx` (register slot)

**Acceptance Criteria:**
- [ ] Badge appears in `STATUS_BAR_LEFT` when active session has a run
- [ ] Click badge → opens drawer
- [ ] Drawer shows: workflow name, current stage / total stages, status, manual controls (Pause / Resume / Skip / Cancel)
- [ ] Manual controls call `controlRun()` and refresh state on success

**Verify:** `npm run dev`, attach a workflow, see badge in status bar, click → drawer opens with correct state.

**Steps:**

- [ ] **Step 1: Badge**

`src/extensions/orchestrator/OrchestratorBadge.tsx`:

```tsx
import React, { useEffect, useState } from 'react';
import { getRunState } from './api';
import type { WorkflowRun } from '../../types/workflow';

type Props = { sessionId: string | null; onOpen: () => void };

export function OrchestratorBadge({ sessionId, onOpen }: Props) {
  const [run, setRun] = useState<WorkflowRun | null>(null);

  useEffect(() => {
    if (!sessionId) { setRun(null); return; }
    let cancelled = false;
    const tick = async () => {
      try {
        const s = await getRunState(sessionId);
        if (!cancelled) setRun(s?.run ?? null);
      } catch {}
    };
    tick();
    const id = setInterval(tick, 2000);
    return () => { cancelled = true; clearInterval(id); };
  }, [sessionId]);

  if (!run) return null;

  return (
    <button
      onClick={onOpen}
      style={{ padding: '2px 8px', fontSize: 12, background: '#2a2a2a', color: '#eee', border: '1px solid #444', borderRadius: 3 }}
    >
      ▶ {run.workflow_id} · {run.current_stage_id} ({run.status})
    </button>
  );
}
```

- [ ] **Step 2: RunDrawer**

`src/extensions/orchestrator/RunDrawer.tsx`:

```tsx
import React, { useEffect, useState } from 'react';
import { controlRun, getRunState, listWorkflows } from './api';
import type { Workflow, WorkflowRun } from '../../types/workflow';

type Props = { sessionId: string; onClose: () => void };

export function RunDrawer({ sessionId, onClose }: Props) {
  const [run, setRun] = useState<WorkflowRun | null>(null);
  const [workflow, setWorkflow] = useState<Workflow | null>(null);

  const reload = async () => {
    const s = await getRunState(sessionId);
    setRun(s?.run ?? null);
    if (s?.run) {
      const ws = await listWorkflows();
      setWorkflow(ws.find(w => w.id === s.run.workflow_id) ?? null);
    }
  };
  useEffect(() => { reload(); }, [sessionId]);

  if (!run) return null;

  const action = async (a: 'pause' | 'resume' | 'cancel' | 'skip') => {
    await controlRun(sessionId, a);
    await reload();
  };

  return (
    <aside style={drawerStyle}>
      <header style={{ display: 'flex', justifyContent: 'space-between' }}>
        <h3>{workflow?.name ?? run.workflow_id}</h3>
        <button onClick={onClose}>×</button>
      </header>
      <p>Status: <strong>{run.status}</strong></p>

      <ol>
        {workflow?.stage_order.map(sid => (
          <li key={sid} style={{ fontWeight: sid === run.current_stage_id ? 'bold' : 'normal' }}>
            {sid} {sid === run.current_stage_id && '← current'}
          </li>
        ))}
      </ol>

      <div style={{ display: 'flex', gap: 8, marginTop: 16 }}>
        {run.status === 'running' && <button onClick={() => action('pause')}>Pause</button>}
        {run.status === 'paused' && <button onClick={() => action('resume')}>Resume</button>}
        <button onClick={() => action('skip')}>Skip stage</button>
        <button onClick={() => { if (confirm('Cancel run?')) action('cancel'); }} style={{ color: 'crimson' }}>Cancel</button>
      </div>
    </aside>
  );
}

const drawerStyle: React.CSSProperties = {
  position: 'fixed', top: 0, right: 0, bottom: 0, width: 360,
  background: '#1e1e1e', color: '#eee', padding: 16, borderLeft: '1px solid #333', overflow: 'auto', zIndex: 999,
};
```

- [ ] **Step 3: Register in slot**

In `src/extensions/orchestrator/index.tsx`:

```tsx
ctx.registerComponent({
  slot: 'STATUS_BAR_LEFT',
  component: () => {
    const [open, setOpen] = useState(false);
    const sid = ctx.sessions.activeSessionId();
    return (
      <>
        <OrchestratorBadge sessionId={sid} onOpen={() => setOpen(true)} />
        {open && sid && <RunDrawer sessionId={sid} onClose={() => setOpen(false)} />}
      </>
    );
  },
});
```

⚠️ NOTE: adapt slot name and component-registration shape to existing `ComponentRegistry` API (see `src/kernel/ComponentRegistry`). Existing extensions like `policy-badge` are good reference.

- [ ] **Step 4: Manual verify + commit**

```bash
npm run dev
# Attach workflow, see badge, click, see drawer, hit Pause → status updates.
git add src/extensions/orchestrator/
git commit -m "feat(ui): status badge + run drawer with manual controls"
```

---

## Task 20: Feature flag + example workflow + docs

**Goal:** Gate the entire orchestrator behind a config flag (default off in v1.5.0). Ship one example workflow with the app. Update README + CHANGELOG.

**Files:**
- Modify: `src/kernel/ConfigProvider.tsx` (add `orchestrator.enabled` default)
- Modify: `crates/extensions/orchestrator/src/lib.rs` (skip activation when disabled)
- Create: `config/example-workflow.md` (shipped resource)
- Modify: `src-tauri/src/main.rs` (copy example workflow to `~/.claude-tabs/workflows/` on first run)
- Modify: `README.md`
- Modify: `CHANGELOG.md`

**Acceptance Criteria:**
- [ ] When `orchestrator.enabled === false`, no HTTP listener is started; UI surfaces hidden
- [ ] First run with feature on: example workflow appears in library
- [ ] README has a "Workflows (beta)" section with quickstart
- [ ] CHANGELOG lists v1.5.0 with workflow orchestration entry

**Verify:** Toggle the setting in Settings UI → orchestrator activates / deactivates without crash; example workflow visible after first launch.

**Steps:**

- [ ] **Step 1: Config default**

In the existing ConfigProvider, add default `orchestrator.enabled: false`. Mirror the pattern of `autoAccept.enabled`.

- [ ] **Step 2: Gate Rust activation**

In `crates/extensions/orchestrator/src/lib.rs`'s `activate`:

```rust
let enabled = ctx.config.get_bool("orchestrator.enabled").unwrap_or(false);
if !enabled {
    tracing::info!("orchestrator disabled by config");
    return Ok(());
}
// ...rest of activation
```

(Adapt `ctx.config.get_bool` to the actual `Config` API.)

- [ ] **Step 3: Example workflow**

`config/example-workflow.md`:

```markdown
---
id: example-ship-feature
name: Example — ship a feature end-to-end
description: Implement → compact → review
inputs:
  - name: task
    description: What feature to ship
    required: true
    type: string
---

## Stage: implement
goal: Build the feature including PRs.
prompt: |
  Please complete {{task}} all the way through creating PRs.
completion:
  type: criteria
  criteria: PRs are created and pushed; agent has stopped iterating on code.
next: compact

## Stage: compact
prompt: |
  /compact After compaction, we will review the codebase to assess release readiness.
completion:
  type: session_event
  on: compact
next: review

## Stage: review
prompt: |
  Awesome work so far. For each PR, please review the code.
notes:
  type: dynamic
  hint: Remind the agent that {{task}} should keep docs in sync. Mention any PRs lacking tests.
completion:
  type: criteria
  criteria: Each PR has been reviewed with verdict and concrete findings.
next: done
```

- [ ] **Step 4: First-run copy**

Add to Tauri `setup` (or wherever Tab does first-run init):

```rust
let cfg_dir = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
    .join(".claude-tabs/workflows");
std::fs::create_dir_all(&cfg_dir).ok();
let example_path = cfg_dir.join("example-ship-feature.md");
if !example_path.exists() {
    let bundled = include_str!("../../config/example-workflow.md");
    let _ = std::fs::write(&example_path, bundled);
}
```

- [ ] **Step 5: README section**

Append to `README.md`:

```markdown
## Workflows (beta, v1.5.0)

Define multi-stage agent workflows in Markdown and attach them to sessions for automated progression.

**Quickstart:**

1. Install the orchestrator plugin in Claude Code:
   ```
   /plugin marketplace add MjMoshiri/claude-tabs-orchestrator
   /plugin install claude-tabs-orchestrator@claude-tabs-orchestrator
   ```
2. Enable the feature: Settings → "Workflow orchestration (beta)" → on.
3. Restart any open Claude Code sessions so the plugin loads.
4. Cmd+K → "Attach workflow to active session" → pick `example-ship-feature` → fill the `task` input → start.
5. The agent will receive the first stage's prompt automatically; subsequent stages advance when the supervisor judges completion.

Workflow files live at `~/.claude-tabs/workflows/*.md`. See the bundled example for the format.

**Status:** beta. Linear stage chains supported in v1; branching and cross-session workflows planned for v2.
```

- [ ] **Step 6: CHANGELOG**

Prepend to `CHANGELOG.md`:

```markdown
## v1.5.0 — Workflow orchestration (beta)

- Add workflow orchestration: attach a Markdown-defined multi-stage workflow to a session; the supervisor advances stages automatically by judging completion via the `claude -p` CLI.
- New companion plugin `claude-tabs-orchestrator` ships separately. Both are required for orchestration to function; without them, sessions behave normally.
- Hidden behind `orchestrator.enabled` flag (default off). Toggle in Settings.
- Bundled example workflow: `example-ship-feature.md`.
```

- [ ] **Step 7: Commit**

```bash
git add config/example-workflow.md README.md CHANGELOG.md src-tauri/ src/kernel/ crates/extensions/orchestrator/
git commit -m "feat: feature flag, example workflow, README, CHANGELOG for v1.5.0 beta"
```

---

## Task 21: E2E smoke test (manual checklist)

**Goal:** Confirm the entire stack works end-to-end before tagging v1.5.0.

**Files:**
- Create: `docs/superpowers/specs/2026-05-07-workflow-orchestration-smoke-checklist.md`

**Acceptance Criteria (each verified by hand):**
- [ ] Fresh Tab dev build launches; `~/.claude-tabs/orchestrator.{port,token}` files appear
- [ ] Plugin installed in Claude Code via `/plugin install`; new session triggers SessionStart hook (verify Tab log)
- [ ] Settings → toggle orchestrator on → restart → port file appears
- [ ] Library lists `example-ship-feature`
- [ ] Cmd+K → Attach → pick example → fill `task` → submit
- [ ] PTY shows the rendered stage 1 prompt typed in
- [ ] Agent does work, finishes turn → Stop hook fires → Tab log shows `turn_end` POST received → judge call made
- [ ] If judge says "complete" → next stage prompt appears in PTY
- [ ] `/compact` advances the `compact` stage via SessionStart `source=compact`
- [ ] Full chain reaches `done`; status badge updates; run drawer shows status `done`
- [ ] Pause / resume / skip / cancel each work
- [ ] With Tab closed: hook curl fails silently; `claude` continues to work normally (no orchestration, no error)

**Verify:** All items above checked.

**Steps:**

- [ ] **Step 1: Run the matrix**

Walk through every checkbox in order. For each, log result + screenshot in the smoke checklist file.

- [ ] **Step 2: Triage failures**

For any failure, file an issue (or fix immediately if obvious). Re-run the affected portion.

- [ ] **Step 3: Tag**

When all green:

```bash
# In Tab repo
git tag v1.5.0-beta
# In claude-tabs-orchestrator repo
git tag v0.1.0  # already done in Task 15
```

- [ ] **Step 4: Commit checklist**

```bash
git add docs/superpowers/specs/2026-05-07-workflow-orchestration-smoke-checklist.md
git commit -m "docs: v1.5.0 smoke checklist results"
```

---

## Self-review (post-write)

Spec coverage:

- [x] Section 3 DSL — Tasks 2 (IR), 3 (parser), 4 (compiler), 5 (loader)
- [x] Section 4 plugin — Task 15
- [x] Section 5 transport — Tasks 13 (auth), 14 (server)
- [x] Section 6 core — Tasks 6 (FSM), 7 (store), 8 (render), 9 (transcript), 10 (guardrails), 11 (judges), 12 (injector)
- [x] Section 7 persistence — Task 1 (migrations)
- [x] Section 8 UI — Tasks 16 (bridge), 17 (library), 18 (attach), 19 (badge + drawer)
- [x] Section 9 failure modes — Tasks 10 (loop break), 14 (token check, hook timeout), 11 (judge errors), 7 (UNIQUE constraint)
- [x] Section 10 observability — Task 7 (`stage_history.judge_log`); orchestrator.log file is wired through `tracing::info!` calls in dispatcher
- [x] Section 11 hard guardrails — Task 10
- [x] Section 12 MVP cut — All v1 items present; deferred items not in plan
- [x] Section 13 release plan — Tasks 20 (feature flag, docs), 21 (smoke + tag)

No placeholders found. Type/method names match across tasks.

One known soft spot: Task 12 (PTY injector) and Task 14 (extension activation) reference `PtyManager::write` and `SessionStore::pty_manager_arc()` — exact API names need adaptation to whatever the existing crates expose. Steps include a note to that effect.
