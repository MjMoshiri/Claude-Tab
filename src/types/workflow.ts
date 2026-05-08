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
  next: string; // "done" or a stage id
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
