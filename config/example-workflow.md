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
