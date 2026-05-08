import { useEffect, useState } from 'react';
import { getRunState } from './api';
import type { WorkflowRun } from '../../types/workflow';

type Props = { sessionId: string | null; onOpen: () => void };

export function OrchestratorBadge({ sessionId, onOpen }: Props) {
  const [run, setRun] = useState<WorkflowRun | null>(null);

  useEffect(() => {
    if (!sessionId) {
      setRun(null);
      return;
    }
    let cancelled = false;
    const tick = async () => {
      try {
        const s = await getRunState(sessionId);
        if (!cancelled) setRun(s?.run ?? null);
      } catch {
        // network blips ignored
      }
    };
    tick();
    const id = setInterval(tick, 2000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [sessionId]);

  if (!run) return null;

  return (
    <button
      onClick={onOpen}
      title={`Run ${run.run_id} — ${run.status}`}
      style={{
        padding: '2px 8px',
        fontSize: 12,
        background: '#2a2a2a',
        color: '#eee',
        border: '1px solid #444',
        borderRadius: 3,
        cursor: 'pointer',
      }}
    >
      &#9654; {run.workflow_id} &middot; {run.current_stage_id} ({run.status})
    </button>
  );
}
