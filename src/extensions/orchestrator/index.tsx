import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { FrontendExtension } from '../../types/extension';
import { SLOTS } from '../../types/slots';
import { OrchestratorBadge } from './OrchestratorBadge';
import { RunDrawer } from './RunDrawer';

/**
 * OrchestratorBar — wraps the badge + drawer, tracks the active session id
 * by listening to core-event (same pattern as PolicyBadge / ActiveSessionInfo).
 * Registered into STATUS_BAR_CENTER so it appears in the footer status bar.
 */
function OrchestratorBar() {
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    let mounted = true;

    invoke<string | null>('get_active_session').then((id) => {
      if (mounted) setSessionId(id);
    });

    let unsub: (() => void) | null = null;
    listen<{ topic: string; payload: Record<string, unknown> }>(
      'core-event',
      (e) => {
        if (!mounted) return;
        const { topic, payload } = e.payload;
        if (topic === 'session.active_changed' || topic === 'session.created') {
          setSessionId(payload.session_id as string);
          // Close drawer if session changes so stale run isn't shown
          setOpen(false);
        }
        if (topic === 'session.closed') {
          setSessionId((cur) =>
            cur === (payload.session_id as string) ? null : cur,
          );
          setOpen(false);
        }
      },
    ).then((u) => {
      unsub = u;
    });

    return () => {
      mounted = false;
      if (unsub) unsub();
    };
  }, []);

  return (
    <>
      <OrchestratorBadge sessionId={sessionId} onOpen={() => setOpen(true)} />
      {open && sessionId && (
        <RunDrawer sessionId={sessionId} onClose={() => setOpen(false)} />
      )}
    </>
  );
}

export function createOrchestratorExtension(): FrontendExtension {
  return {
    manifest: {
      id: 'orchestrator',
      name: 'Orchestrator',
      version: '0.1.0',
      description: 'Workflow orchestration badge and run drawer',
    },
    activate(ctx) {
      ctx.componentRegistry.register(SLOTS.STATUS_BAR_CENTER, {
        id: 'orchestrator-badge',
        component: OrchestratorBar,
        priority: 10,
        extensionId: 'orchestrator',
      });
    },
  };
}
