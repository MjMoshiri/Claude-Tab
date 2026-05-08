# Workflow Orchestration — Design Spec

- **Date:** 2026-05-07
- **Status:** Approved (brainstorming → writing-plans handoff pending)
- **Owner:** mjmoshiri
- **Target version:** Claude-Tab v1.5.0 (behind `orchestrator.enabled` feature flag)

## 1. Goal

Provide an extensible workflow orchestration layer for Claude-Tab sessions. A user attaches a defined workflow to a session; the orchestrator advances the agent through pre-authored stages automatically, applying contextual notes when useful, and surfaces state in the UI.

The system is opinionated about safety: a deterministic finite state machine provides the rails; LLM judgments are scoped to small, atomic, testable questions ("did this stage end?", "which declared branch should we take?", "what addendum belongs on this prompt?"). The LLM never holds the steering wheel.

## 2. Architecture overview

Hybrid: FSM rails + LLM judges + DSL extension points. Two components:

- **Plugin** (`claude-tabs-orchestrator`) — Claude Code plugin shipped via the user's existing marketplace. Two thin hook scripts: `Stop` and `SessionStart`. Each posts to Claude-Tab over localhost HTTP and exits.
- **Tab orchestrator extension** (`crates/extensions/orchestrator`) — runs a Tokio + axum HTTP listener, owns workflow state in SQLite, runs the supervisor LLM, and injects the next stage's prompt into the running session via PTY auto-typing.

Both components are mandatory. If either is missing, the system is a no-op (the agent runs normally; runs are not advanced).

```
┌──────────────────────┐   Stop / SessionStart hook        ┌────────────────────────────┐
│  Claude Code session │ ────────────► POST /turn-end ───► │  Tab orchestrator          │
│  (PTY child of Tab)  │                                   │   - HTTP listener (axum)   │
│                      │ ◄────── PTY auto-type next ────── │   - FSM advance (pure fn)  │
└──────────────────────┘                                   │   - Judges (claude -p)     │
                                                           │   - SQLite store           │
                                                           │   - UI state events        │
                                                           └────────────────────────────┘
```

Trigger model: **one signal — Stop hook on every turn end.** Tab decides what (if anything) happens next. SessionStart is auxiliary (resume / `/compact` detection). No PTY-idle detection, no separate wake-up arbitration channel; if an agent self-schedules a wake-up, that produces another turn, another Stop hook, another supervisor pass.

## 3. Workflow DSL

### 3.1 File layout

Workflows live as Markdown files at `~/.claude-tabs/workflows/<workflow_id>.md`. Markdown is the source of truth; the Tab UI offers CRUD over this directory.

### 3.2 Format

```markdown
---
id: ship-feature
name: Ship a feature end-to-end
description: Implement → review → release-ready
model: claude-haiku-4-5         # supervisor judge model (default haiku)
default_completion: criteria
default_notes: static
default_injection: async
inputs:
  - name: task
    description: What feature/work should the agent ship?
    required: true
    type: string
  - name: pr_count_hint
    description: Approximate PR count (helps reviewer stage)
    required: false
    type: integer
    default: 1
---

## Stage: implement
goal: Build the feature including PRs.
prompt: |
  Please complete {{task}} all the way through creating PRs.
  Expect roughly {{pr_count_hint}} PR(s).
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
  Awesome work so far. For each PR provided, please review the code.
notes:
  type: dynamic
  hint: Remind agent that {{task}} should keep docs in sync. Mention any PRs that lack tests.
completion:
  type: criteria
  criteria: Each PR has been reviewed with verdict and concrete findings.
next: done
```

### 3.3 DSL keywords

| Keyword | Values | Purpose |
|---|---|---|
| `inputs[]` (frontmatter) | `{name, description, required, type, default}`; `type` ∈ `string`, `integer`, `bool`, `enum`, `multiline_string` | Schema for runtime variables, asked at attach time |
| `prompt` | string \| `{file: <path>}` | Authored prompt (verbatim, with `{{var}}` substitution) |
| `completion.type` | `criteria` (LLM yes/no), `tool_use_match` (transcript match), `session_event` (`compact`/`clear`/`resume`) | How stage completion is detected |
| `completion.criteria` | string (only when `type: criteria`) | Natural-language goal the LLM judges against |
| `completion.match` | `{tool: <name>, input: {<field>: <regex>}}` (only when `type: tool_use_match`) | Matched against tool_use entries in the JSONL transcript tail |
| `completion.on` | `compact` \| `clear` \| `resume` (only when `type: session_event`) | Which SessionStart `source` value advances the stage |
| `next` | `<stage_id>` \| `done` | Static successor |
| `branches[]` | `[{to: <stage_id>, when: <criteria>}]` (mutually exclusive with `next`) | LLM picks among declared paths (v2; declare keyword reserved in v1) |
| `notes.type` | `static` (none) \| `dynamic` (LLM drafts addendum from `hint`) | Optional addendum appended to the rendered prompt |
| `model` (per-stage) | overrides workflow `model` | Use a stronger model for hard judgments |
| `default_injection` | `async` \| `sync` | v1 = `async` only; `sync` reserved for v2 |
| `outputs[]` (per-stage, reserved v2) | `[{name, extract: <criteria>}]` | Cross-stage variable passing |
| `on_failure` (reserved v2) | `retry` \| `escalate` \| `branch_to` | Failure-path routing |

Templating: `{{var}}` substitution applies to `prompt`, `notes.hint`, `criteria`, and `branches[].when`. Values are inserted as plain text (no markdown rendering inside the substitution).

### 3.4 Compile-time validation

The MD-to-FSM compiler (`crates/extensions/orchestrator/src/workflow/compiler.rs`) rejects a workflow file at load time if any of the following fail:

- All `next` and `branches[].to` references resolve to declared stage ids
- Every `{{var}}` reference resolves to a declared `inputs[].name` (or, in v2+, `stages.<id>.outputs.<name>`)
- Exactly one path reaches `done`
- No two stages share an id
- `completion.type` value is in the supported set
- `model` value is in the allowed set
- Frontmatter required fields (`id`, `name`) present and non-empty

Invalid workflows are excluded from the library with an error visible in the Tab UI; valid workflows are unaffected.

### 3.5 Run state

```
WorkflowRun {
  run_id, session_id, workflow_id, started_at,
  inputs: { ...substituted-once at attach time... },
  current_stage_id,
  history: [ { stage_id, entered_at, exited_at, verdict, judge_calls[] } ],
  status: running | paused | done | escalated | error
}
```

Inputs are locked at run start. Mid-run input mutation is not supported in v1 (would invalidate stage-1 history).

## 4. Plugin (`claude-tabs-orchestrator`)

### 4.1 Layout

```
claude-tabs-orchestrator/
├── .claude-plugin/
│   ├── plugin.json
│   └── marketplace.json
└── hooks/
    ├── hooks.json
    ├── on_stop.sh
    └── on_session_start.sh
```

### 4.2 hooks.json

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

### 4.3 on_stop.sh (sketch)

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

### 4.4 on_session_start.sh (sketch)

Same shape; POSTs `{event:"session_start", session_id, source}` to `/session-start`. The `source` field (`startup` / `resume` / `clear` / `compact`) drives Tab behavior:

- `startup` → if profile carries a workflow id, prompt for inputs, attach run, PTY-type stage 1
- `resume` → reattach existing run from SQLite
- `clear` → run is paused; user resumes manually from the run drawer
- `compact` → workflow continues; first-class trigger for stages with `completion.type: session_event, on: compact`

Hook scripts deliberately have no logic beyond signaling. All decisions live in Tab.

## 5. Transport: localhost HTTP

| Aspect | Choice | Reason |
|---|---|---|
| Server | Tokio + axum, embedded in the orchestrator extension | Tab already hosts a Tokio runtime |
| Bind | `127.0.0.1:<auto>`, port chosen by OS | Localhost-only; no LAN exposure |
| Port discovery | Tab writes `~/.claude-tabs/orchestrator.port` on startup, deletes on shutdown | Mirrors the existing `auto-accept-policies/` pattern |
| Auth | 32-byte random hex token at `~/.claude-tabs/orchestrator.token` (mode `0600`), sent in `X-CT-Token` header, rotated each Tab launch | Prevents stray local processes from steering sessions |
| Response shape | 204 No Content on success; empty body | Hook does not need to forward anything to Claude Code in v1 (async injection only) |
| Timeouts | Hook curl `--max-time 8`; Claude Code hook `timeout 10` | Tab returns 204 fast; supervisor work is enqueued and runs after the hook exits |
| macOS sandbox | Unsigned/dev build is fine. Sandboxed App-Store distribution would require `com.apple.security.network.server` entitlement | Defer entitlement until release-channel ships |

Endpoints:

- `POST /turn-end` — `{event, session_id, transcript_path}` → enqueue evaluation, return 204
- `POST /session-start` — `{event, session_id, source}` → reattach / reset / advance per policy
- `POST /workflow/attach` — internal (called by Tab UI itself)
- `GET /workflow/state/<session_id>` — UI status

## 6. Orchestrator core

### 6.1 Crate layout

```
crates/extensions/orchestrator/
├── Cargo.toml
└── src/
    ├── lib.rs                 # Extension impl
    ├── workflow/
    │   ├── parser.rs          # MD frontmatter + ## Stage blocks → IR
    │   ├── compiler.rs        # IR → FSM, validates references
    │   └── ir.rs              # Workflow / Stage / Completion / Inputs
    ├── runtime/
    │   ├── run.rs             # WorkflowRun, pure advance fn
    │   ├── store.rs           # SQLite I/O
    │   └── attach.rs          # Bind run to session_id
    ├── judge/
    │   ├── completion.rs      # criteria | tool_use_match | session_event
    │   ├── branch.rs          # next:auto / branches[] (v2)
    │   └── notes.rs           # Dynamic notes addendum
    ├── transport/
    │   ├── server.rs          # axum endpoints
    │   └── auth.rs            # token check
    ├── transcript.rs          # JSONL reader, tool_use extractor
    └── injector.rs            # PTY write
```

### 6.2 Pure FSM advance

```rust
fn advance(run: &WorkflowRun, workflow: &Workflow, verdict: JudgeVerdict)
    -> AdvanceOutcome
{
    match verdict {
        Verdict::StayInStage          => Outcome::NoOp,
        Verdict::Complete { next }     => match resolve_next(workflow, run.current_stage_id, next) {
            StageRef::Done       => Outcome::Done,
            StageRef::Stage(id)  => Outcome::Inject {
                stage_id: id,
                rendered_prompt: render(workflow, id, &run.inputs),
            },
        },
        Verdict::Escalate { reason }   => Outcome::Escalate(reason),
    }
}
```

Pure: no I/O, fully unit-testable on `(run, workflow, verdict)` triples.

### 6.3 Per-turn dispatcher

Both endpoints share the same advance-and-inject helper; they differ only in how the verdict is produced.

```
on POST /turn-end (session_id, transcript_path):
  run     = store.load_run(session_id)        -- None? return 204 (no workflow attached)
  stage   = workflow.stages[run.current_stage_id]
  tail    = transcript.tail(transcript_path, last_n=80)

  verdict = match stage.completion.type:
    tool_use_match  -> tail.tool_uses().any(|tu| matches(stage.completion.match, tu))
    session_event   -> false   -- only advanced from /session-start
    criteria        -> llm_judge_completion(stage, run, tail)

  if !verdict.is_complete: return 204
  advance_and_inject(run, stage, tail)
  return 204

on POST /session-start (session_id, source):
  run = store.load_run(session_id)
  if run is None:
    if profile_for(session_id).workflow_id is set and source == "startup":
      run = attach_run(session_id, profile.workflow_id, profile.workflow_inputs)
      advance_to_stage_one(run)
    return 204

  match source:
    "resume"  -> 204                           -- state already loaded
    "clear"   -> store.set_status(run, "paused"); return 204
    "compact" ->
      stage = workflow.stages[run.current_stage_id]
      if stage.completion.type == "session_event" and stage.completion.on == "compact":
        advance_and_inject(run, stage, tail=None)
      return 204

advance_and_inject(run, stage, tail):
  next_id = stage.next  -- v1: static only
  notes   = if stage.notes.is_dynamic
              then llm_draft_notes(stage.notes.hint, run, tail)
              else ""
  rendered = render_prompt(workflow.stages[next_id], run.inputs) + notes
  store.advance_run(run, next_id, judge_log)
  injector.queue_pty_write(session_id, rendered)
```

### 6.4 LLM client

v1: `claude -p` subprocess, mirroring the auto-accept plugin. No API key plumbing; works for every Claude Code user. Slower per call but lowest-friction shipping path.

v2: direct Anthropic Rust SDK with prompt caching. API key resolution: per-workflow env var → Tab keychain (Tauri keyring plugin) → fallback to `claude -p`.

### 6.5 Judge prompt (completion)

```
You are a workflow stage completion judge.

Stage goal: {{stage.goal}}
Completion criteria: {{stage.completion.criteria}}

Recent transcript (last {{N}} entries):
{{transcript_tail_compact}}

Last assistant message (full):
{{last_assistant_msg}}

Has this stage met its completion criteria?

Output strictly:
<answer>{"complete": true|false, "reason": "<≤30 words>"}</answer>
```

### 6.6 Render

```rust
fn render_prompt(stage: &Stage, inputs: &Inputs) -> String {
    let body = substitute(&stage.prompt, inputs);
    if stage.notes.is_dynamic() {
        format!("{}\n\n---\n[Orchestrator note: {}]", body, stage.cached_notes)
    } else {
        body
    }
}
```

### 6.7 Injection

`injector.write_user_prompt(session_id, text)`:

1. Look up PTY handle via `SessionStore`
2. Write `text` followed by `\r` to the PTY master
3. Use existing output-parser primitives to confirm echo / idle
4. On failure: mark run `error`, surface in UI

### 6.8 Transcript reader

`transcript.rs` opens the JSONL file referenced by `transcript_path`, reads from end up to `last_n` entries, parses each line, and exposes `tool_uses()`, `last_assistant_msg()`, and a compact textual representation for prompt embedding.

## 7. Persistence

Schema is additive in the existing `~/.claude-tabs/archive.db`:

```sql
CREATE TABLE workflows (
  id            TEXT PRIMARY KEY,
  name          TEXT NOT NULL,
  description   TEXT,
  source_path   TEXT NOT NULL,           -- ~/.claude-tabs/workflows/<id>.md
  source_hash   TEXT NOT NULL,
  compiled_json TEXT NOT NULL,
  updated_at    INTEGER NOT NULL
);

CREATE TABLE workflow_runs (
  run_id            TEXT PRIMARY KEY,
  session_id        TEXT NOT NULL UNIQUE,
  workflow_id       TEXT NOT NULL REFERENCES workflows(id),
  inputs_json       TEXT NOT NULL,
  current_stage_id  TEXT NOT NULL,
  status            TEXT NOT NULL,       -- running|paused|done|escalated|error
  started_at        INTEGER NOT NULL,
  ended_at          INTEGER
);
CREATE INDEX idx_runs_session ON workflow_runs(session_id);

CREATE TABLE stage_history (
  id          INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id      TEXT NOT NULL REFERENCES workflow_runs(run_id),
  stage_id    TEXT NOT NULL,
  entered_at  INTEGER NOT NULL,
  exited_at   INTEGER,
  verdict     TEXT,                       -- complete|stay|escalate|error
  judge_log   TEXT                        -- json: {model, prompt, response, tokens, latency_ms}
);
CREATE INDEX idx_history_run ON stage_history(run_id);
```

MD files remain the source of truth. `workflows` table is a compile cache; on Tab startup, scan `~/.claude-tabs/workflows/*.md`, hash each, recompile if `source_hash` changed.

## 8. UI surfaces

Frontend additions live under `src/extensions/orchestrator/`:

| Surface | Component | Slot | Purpose |
|---|---|---|---|
| Workflow library | `WorkflowsPanel.tsx` | settings tab | List / preview workflows; surface compile errors. v1 is read-only — files edited externally. |
| Stage editor (v2) | `StageEditor.tsx` | inside library | Form-based editing with idempotent MD round-trip |
| Attach picker | `AttachWorkflow.tsx` | tab right-click + command palette | Choose workflow → fill inputs → start run |
| Status badge | `OrchestratorBadge.tsx` | `STATUS_BAR_LEFT` | Compact live state: `▶ ship-feature · stage 2/4 review` |
| Run detail drawer | `RunDrawer.tsx` | overlay | Stage timeline, judge transcripts, manual controls |
| Profile linkage | edit `ProfileEditor.tsx` | existing settings | Add `workflow_id?` + `workflow_inputs?` to profiles for auto-attach (v2) |

Manual controls in the run drawer (all hit Tab HTTP endpoints, no plugin involvement):

- **Pause** — stops PTY injection but keeps run state
- **Resume** — re-runs supervisor on current state
- **Skip stage** — force advance to `next`
- **Force complete** — mark workflow `done`
- **Cancel** — mark `paused`, detach from session
- **Re-run stage** — re-render and re-inject current stage prompt

Existing EventBus topics: `workflow.stage.entered`, `workflow.stage.exited`, `workflow.run.completed`, `workflow.run.errored`. Status badge subscribes; `system-notify` extension can surface OS notifications on completion.

## 9. Failure modes

| Failure | Detection | Recovery |
|---|---|---|
| Plugin not installed | Stop hook never fires; Tab sees no `/turn-end` | Tab UI flags "orchestrator inactive" on attached runs; user can advance manually |
| Tab not running when hook fires | Hook curl fails → exits 0 silently | Agent finishes turn; user resumes manually; stage history records the gap |
| Token mismatch | HTTP 401 from Tab | Hook treats as Tab-not-running; exits 0 |
| Judge LLM call fails / times out | catch + log to `stage_history.judge_log` | Run → `error`; UI shows banner with retry button. No auto-retry to avoid tight loops |
| Judge returns malformed JSON | parse fails | Same as above; raw response logged |
| PTY write fails (session closed mid-advance) | PTY API returns error | Run → `error`; OS notification |
| Workflow MD parse error on Tab startup | compiler returns `Result` | Workflow excluded from library with red error in UI; other workflows unaffected |
| MD edited mid-run (drift) | hash check on every load | Existing run keeps cached IR; edits only affect future attaches |
| Loop: judge keeps advancing rapidly | per-run advance counter; cap = 20 advances/hour | Cap exceeded → run → `escalated`, surface in UI |
| User types into PTY while injection pending | last-writer-wins; injection still queued | Both prompts land in transcript. Acceptable in v1; v2 may add cancel-on-user-input |
| Hook timeout (10s) before Tab responds | hook killed by Claude Code | Tab finishes evaluation async; PTY-types when ready. Same end state. |
| Concurrent runs in same session | `UNIQUE(session_id)` on `workflow_runs` | UI: "session already has workflow X attached" |

## 10. Observability (v1)

- Stage transitions logged to `~/.claude-tabs/logs/orchestrator.log`
- Each judge call records `{run_id, stage_id, model, prompt_tokens, completion_tokens, latency_ms, verdict}` to `stage_history.judge_log`
- Run drawer shows the last 50 history rows, expandable
- Local-only; no telemetry, no metrics export

## 11. Hard guardrails

- Max 20 advances per run per hour (loop break)
- Max judge prompt size 24 KB (truncate transcript tail)
- Max workflow MD file 256 KB
- One workflow run per session at a time

## 12. MVP cut

| Included in v1 | Deferred |
|---|---|
| Linear workflows (`next: <stage_id>` and `next: done`) | `branches[]`, `next: auto` |
| `completion.type`: `criteria`, `tool_use_match`, `session_event` | `regex`, `multi` |
| Async injection (PTY auto-typing) | Sync inline-block via Stop hook `decision: block` |
| `claude -p` subprocess as judge | Direct Anthropic SDK + prompt caching |
| MD source-of-truth + read-only Tab list UI | Form-based stage editor with MD round-trip |
| Manual attach via command palette | Profile auto-attach |
| Dynamic notes (LLM-drafted addendum) | Stage outputs (`{{stages.X.outputs.Y}}`) |
| Pause / Resume / Cancel / Skip stage | Re-run stage, fork run, replay |
| One run per session | Cross-session workflows |
| Status badge + run drawer | OS notifications, sound, telemetry |

## 13. Release plan

1. **Internal dogfood** — install plugin + Tab dev build, run user's own ship-feature workflow against real PR cycles. ≥3 successful end-to-end runs before public release.
2. **Plugin registry** — publish `claude-tabs-orchestrator` to the user's marketplace alongside `auto-accept`. Tag `v0.1.0` (pre-stable).
3. **Tab release** — bump to `v1.5.0`. CHANGELOG: "Workflow orchestration (beta) — attach workflows to sessions for automated multi-stage runs."
4. **Docs** — README section + one example workflow shipped with Tab at `~/.claude-tabs/workflows/example-ship-feature.md`. No public docs site yet.
5. **Feedback gate** — collect issue reports for 1 week before promoting past beta. Bumps to `v1.5.1` if no critical bugs; pulls feature flag if so.
6. **Feature flag** — `orchestrator.enabled` (default `false` in v1.5.0). Users opt in via Settings. Promote to default-on in v1.6.0 once feedback gate passes.

## 14. Roadmap beyond v1

**v2 (within 2–4 weeks of v1 ship):**
- `branches[]` and `next: auto`
- Direct Anthropic SDK with prompt caching
- Stage editor UI with MD round-trip
- Profile auto-attach
- `regex` and `multi` completion types
- Stage outputs (`outputs[]` and `{{stages.X.outputs.Y}}`)

**v3 (later):**
- Cross-session workflows (parent run spawns child sessions)
- Sync inline-block injection mode
- Workflow sharing (gist import / export)
- Replay / fork past runs
- `on_failure: retry | escalate | branch_to`

## 15. Out of scope (will not build)

- LLM-authored workflows (users hand-write MD)
- Multi-tenant or shared runs across users
- Web dashboard
- Anything depending on Claude Code internals not in the public hook docs
