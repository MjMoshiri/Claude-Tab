# v1.5.0-beta Workflow Orchestration — Manual Smoke Checklist

**Branch:** `feat/orchestrator`
**Tag (pending):** `v1.5.0-beta`
**Companion plugin tag:** `claude-tabs-orchestrator v0.1.0` (sibling repo)

Walk through every checkbox in order. Log result + brief notes inline.

## Pre-flight (automated)

- [ ] `cargo test` — all green (run from `/Users/mjmoshiri/Claude-Tab`)
- [ ] `cargo build` — clean
- [ ] `npm run build` — clean
- [ ] `bash /Users/mjmoshiri/claude-tabs-orchestrator/hooks/on_stop.sh` — no-op exits 0 with no port file present

## Bootstrap

- [ ] Toggle `orchestrator.enabled = true` in Tab Settings (or edit `~/.claude-tabs/config.toml` directly).
- [ ] Restart Tab dev build (`npm run dev`).
- [ ] Verify `~/.claude-tabs/orchestrator.port` and `~/.claude-tabs/orchestrator.token` exist; token has mode `0600`.
- [ ] Verify `~/.claude-tabs/workflows/example-ship-feature.md` was copied on first launch.
- [ ] Settings → Workflows → see `example-ship-feature` listed.

## Plugin install

- [ ] In a Claude Code session: `/plugin marketplace add /Users/mjmoshiri/claude-tabs-orchestrator` (or via GitHub URL once pushed).
- [ ] `/plugin install claude-tabs-orchestrator@claude-tabs-orchestrator`.
- [ ] Restart any open sessions so the plugin loads.

## Attach a workflow

- [ ] Settings → Workflows → click "Attach to active session…" (button added in Task 18).
- [ ] Modal opens; pick `example-ship-feature`.
- [ ] Fill `task` input (e.g., "ship the favicon refresh").
- [ ] Click "Start workflow" — modal closes; PTY shows the rendered stage-1 prompt typed in.

## Stage advancement

- [ ] Status bar badge appears: `▶ example-ship-feature · implement (running)`.
- [ ] Click badge → drawer opens; shows workflow name, status, stage timeline with `implement ← current`.
- [ ] Let the agent finish a turn (or stub a fake completion).
- [ ] Stop hook fires → Tab log line: `turn_end error: ...` is OK, `[INFO] injecting next stage` indicates success.
- [ ] If judge votes complete: PTY shows the next stage's prompt auto-typed; current stage in drawer updates.
- [ ] After implement → compact: PTY shows `/compact` prompt; user (or agent) compacts; SessionStart hook fires with `source=compact`; badge updates to `review`.
- [ ] Review stage runs through. Final advance reaches `done`; drawer shows `Status: done`.

## Manual controls

- [ ] Pause: badge updates to `(paused)`; further turns do not advance.
- [ ] Resume: badge back to `(running)`.
- [ ] Skip: drawer's "Skip stage" button → next stage prompt typed; current stage updates.
- [ ] Cancel: confirm dialog → status set to `paused` (no Cancelled variant in v1; this is documented).

## Failure modes

- [ ] Quit Tab. `curl http://127.0.0.1:<port>/turn-end` (using stale port) — fails connection-refused.
- [ ] In the active Claude session, `/plugin` hooks call the dead port → curl with `--max-time` fails silently → `claude` continues uninterrupted.
- [ ] Re-launch Tab; plugin hooks resume working with the new port + token.

## Tag

When every box above is ticked:

```bash
git tag v1.5.0-beta
# Companion plugin tag was already created in Task 15 as v0.1.0.
# Push tags later when you're ready: `git push --tags`.
```

## Known v1 limitations

- Linear workflows only. Branching (`branches[]`, `next: auto`) deferred to v2.
- Cancel maps to Paused (no Cancelled variant).
- No retry on judge errors.
- No streaming UI for stage transitions; badge polls every 2s.
- Stage advancement is async via PTY auto-typing only.
- No outputs; runs are append-only history.

## Notes / failures observed

(leave blank initially; fill in during the walkthrough)
