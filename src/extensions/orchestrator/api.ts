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

export async function getRunState(
  sessionId: string,
): Promise<{ run: WorkflowRun } | null> {
  return invoke('get_run_state', { sessionId });
}

export async function controlRun(
  sessionId: string,
  action: 'pause' | 'resume' | 'cancel' | 'skip',
): Promise<void> {
  return invoke('control_run', { sessionId, action });
}
