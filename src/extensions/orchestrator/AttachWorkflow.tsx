import React, { useEffect, useState } from 'react';
import { attachWorkflow, listWorkflows } from './api';
import type { Workflow } from '../../types/workflow';
import { InputsForm } from './InputsForm';

type Props = { sessionId: string; onClose: () => void };

export function AttachWorkflow({ sessionId, onClose }: Props) {
  const [workflows, setWorkflows] = useState<Workflow[]>([]);
  const [picked, setPicked] = useState<Workflow | null>(null);
  const [loadErr, setLoadErr] = useState<string | null>(null);

  useEffect(() => {
    listWorkflows()
      .then(setWorkflows)
      .catch((e) => setLoadErr(String(e)));
  }, []);

  return (
    <div style={modalBackdrop} onClick={onClose}>
      <div style={modalBody} onClick={(e) => e.stopPropagation()}>
        <header style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          <h3 style={{ margin: 0 }}>Attach workflow to session</h3>
          <button onClick={onClose} aria-label="Close" style={{ fontSize: 20 }}>×</button>
        </header>
        <div style={{ fontSize: 12, opacity: 0.6, marginBottom: 12 }}>
          Session: <code>{sessionId}</code>
        </div>

        {loadErr && <div style={{ color: 'crimson' }}>{loadErr}</div>}

        {!picked && workflows.length === 0 && !loadErr && (
          <div style={{ opacity: 0.6, marginTop: 16 }}>
            No workflows installed. Add one at{' '}
            <code>~/.claude-tabs/workflows/&lt;id&gt;.md</code>.
          </div>
        )}

        {!picked && workflows.length > 0 && (
          <ul style={{ listStyle: 'none', padding: 0, marginTop: 8 }}>
            {workflows.map((w) => (
              <li
                key={w.id}
                style={{
                  padding: 10,
                  cursor: 'pointer',
                  border: '1px solid #444',
                  borderRadius: 4,
                  marginBottom: 6,
                }}
                onClick={() => setPicked(w)}
              >
                <strong>{w.name}</strong>{' '}
                <small style={{ opacity: 0.6 }}>({w.id})</small>
                <div style={{ fontSize: 12, opacity: 0.7 }}>{w.description}</div>
              </li>
            ))}
          </ul>
        )}

        {picked && (
          <>
            <button onClick={() => setPicked(null)} style={{ marginBottom: 12 }}>
              ← back
            </button>
            <h4 style={{ marginTop: 0 }}>{picked.name}</h4>
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
  position: 'fixed',
  top: 0,
  left: 0,
  right: 0,
  bottom: 0,
  background: 'rgba(0,0,0,0.5)',
  display: 'flex',
  alignItems: 'center',
  justifyContent: 'center',
  zIndex: 1000,
};

const modalBody: React.CSSProperties = {
  background: '#1e1e1e',
  color: '#eee',
  padding: 24,
  borderRadius: 8,
  width: 480,
  maxHeight: '80vh',
  overflow: 'auto',
};
