import React, { useEffect, useState } from 'react';
import { controlRun, getRunState, listWorkflows } from './api';
import type { Workflow, WorkflowRun } from '../../types/workflow';

type Props = { sessionId: string; onClose: () => void };

export function RunDrawer({ sessionId, onClose }: Props) {
  const [run, setRun] = useState<WorkflowRun | null>(null);
  const [workflow, setWorkflow] = useState<Workflow | null>(null);
  const [busy, setBusy] = useState(false);

  const reload = async () => {
    const s = await getRunState(sessionId);
    setRun(s?.run ?? null);
    if (s?.run) {
      const ws = await listWorkflows();
      setWorkflow(ws.find((w) => w.id === s.run.workflow_id) ?? null);
    }
  };

  useEffect(() => {
    reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);

  if (!run) {
    return null;
  }

  const action = async (a: 'pause' | 'resume' | 'cancel' | 'skip') => {
    setBusy(true);
    try {
      await controlRun(sessionId, a);
      await reload();
    } finally {
      setBusy(false);
    }
  };

  return (
    <aside style={drawerStyle}>
      <header style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <h3 style={{ margin: 0 }}>{workflow?.name ?? run.workflow_id}</h3>
        <button onClick={onClose} aria-label="Close" style={{ fontSize: 18, background: 'none', border: 'none', color: '#eee', cursor: 'pointer' }}>
          &times;
        </button>
      </header>

      <p style={{ marginTop: 12 }}>
        Status: <strong>{run.status}</strong>
      </p>
      <p style={{ fontSize: 12, opacity: 0.7 }}>
        Run id: <code>{run.run_id}</code>
      </p>

      <ol style={{ paddingLeft: 18 }}>
        {workflow?.stage_order.map((sid) => (
          <li
            key={sid}
            style={{
              fontWeight: sid === run.current_stage_id ? 'bold' : 'normal',
              marginBottom: 4,
            }}
          >
            {sid} {sid === run.current_stage_id && '← current'}
          </li>
        ))}
      </ol>

      <div style={{ display: 'flex', gap: 8, marginTop: 16, flexWrap: 'wrap' }}>
        {run.status === 'running' && (
          <button disabled={busy} onClick={() => action('pause')}>
            Pause
          </button>
        )}
        {run.status === 'paused' && (
          <button disabled={busy} onClick={() => action('resume')}>
            Resume
          </button>
        )}
        <button disabled={busy} onClick={() => action('skip')}>
          Skip stage
        </button>
        <button
          disabled={busy}
          onClick={() => {
            if (confirm('Cancel run?')) action('cancel');
          }}
          style={{ color: 'crimson' }}
        >
          Cancel
        </button>
      </div>
    </aside>
  );
}

const drawerStyle: React.CSSProperties = {
  position: 'fixed',
  top: 0,
  right: 0,
  bottom: 0,
  width: 360,
  background: '#1e1e1e',
  color: '#eee',
  padding: 16,
  borderLeft: '1px solid #333',
  overflow: 'auto',
  zIndex: 999,
};
