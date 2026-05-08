import { useEffect, useState } from 'react';
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
    <div style={{ padding: '4px 0' }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 8 }}>
        <span className="settings-item-desc">
          Files in <code>~/.claude-tabs/workflows/*.md</code>. Edit with any text editor; reload to pick up changes.
        </span>
        <button
          className="settings-skill-group-add-btn"
          onClick={reload}
          disabled={loading}
          style={{ fontSize: 11, padding: '4px 10px', whiteSpace: 'nowrap', marginLeft: 8 }}
        >
          {loading ? 'Loading...' : 'Reload'}
        </button>
      </div>

      {error && (
        <div style={{ color: 'crimson', marginTop: 4, fontSize: 12 }}>
          Error: {error}
        </div>
      )}

      {workflows.length === 0 && !loading && !error && (
        <div className="settings-item-desc" style={{ marginTop: 8 }}>
          No workflows found. Create one at{' '}
          <code>~/.claude-tabs/workflows/&lt;id&gt;.md</code>.
        </div>
      )}

      {workflows.length > 0 && (
        <ul style={{ listStyle: 'none', padding: 0, margin: '8px 0 0 0' }}>
          {workflows.map((w) => (
            <li
              key={w.id}
              style={{
                border: '1px solid #333',
                padding: '8px 12px',
                marginBottom: 6,
                borderRadius: 4,
              }}
            >
              <div style={{ fontWeight: 'bold', fontSize: 13 }}>{w.name}</div>
              <div className="settings-item-desc" style={{ fontSize: 11, marginTop: 1 }}>{w.id}</div>
              {w.description && (
                <div style={{ marginTop: 3, fontSize: 12 }}>{w.description}</div>
              )}
              <div className="settings-item-desc" style={{ marginTop: 3, fontSize: 11 }}>
                {w.stage_order.length} stage{w.stage_order.length !== 1 ? 's' : ''} &middot; model: {w.model}
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
