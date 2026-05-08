import React, { useState } from 'react';
import type { InputSpec } from '../../types/workflow';

type Props = {
  inputs: InputSpec[];
  onSubmit: (values: Record<string, unknown>) => void | Promise<void>;
  submitLabel?: string;
};

export function InputsForm({ inputs, onSubmit, submitLabel = 'Start' }: Props) {
  const [values, setValues] = useState<Record<string, unknown>>(() =>
    Object.fromEntries(inputs.map((i) => [i.name, i.default ?? '']))
  );
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  const missing = inputs.filter(
    (i) => i.required && (values[i.name] === '' || values[i.name] == null)
  );
  const canSubmit = missing.length === 0 && !busy;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setErr(null);
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
      {inputs.map((spec) => (
        <label key={spec.name} style={{ display: 'block', marginBottom: 12 }}>
          <div style={{ fontWeight: 'bold' }}>
            {spec.name}
            {spec.required && <span style={{ color: 'crimson' }}> *</span>}
          </div>
          <div style={{ fontSize: 12, opacity: 0.7 }}>{spec.description}</div>
          {spec.type === 'multiline_string' ? (
            <textarea
              value={String(values[spec.name] ?? '')}
              onChange={(e) =>
                setValues((v) => ({ ...v, [spec.name]: e.target.value }))
              }
              rows={4}
              style={{ width: '100%', marginTop: 4 }}
            />
          ) : spec.type === 'enum' ? (
            <select
              value={String(values[spec.name] ?? '')}
              onChange={(e) =>
                setValues((v) => ({ ...v, [spec.name]: e.target.value }))
              }
              style={{ width: '100%', marginTop: 4 }}
            >
              <option value="">—</option>
              {(spec.enum ?? []).map((opt) => (
                <option key={opt} value={opt}>
                  {opt}
                </option>
              ))}
            </select>
          ) : spec.type === 'bool' ? (
            <input
              type="checkbox"
              checked={Boolean(values[spec.name])}
              onChange={(e) =>
                setValues((v) => ({ ...v, [spec.name]: e.target.checked }))
              }
              style={{ marginTop: 4 }}
            />
          ) : (
            <input
              type={spec.type === 'integer' ? 'number' : 'text'}
              value={String(values[spec.name] ?? '')}
              onChange={(e) =>
                setValues((v) => ({
                  ...v,
                  [spec.name]:
                    spec.type === 'integer' ? Number(e.target.value) : e.target.value,
                }))
              }
              style={{ width: '100%', marginTop: 4 }}
            />
          )}
        </label>
      ))}
      {err && <div style={{ color: 'crimson', marginBottom: 8 }}>{err}</div>}
      <button type="submit" disabled={!canSubmit}>
        {busy ? 'Submitting…' : submitLabel}
      </button>
    </form>
  );
}
