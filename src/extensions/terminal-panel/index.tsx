import { useEffect, useState, useCallback, useRef } from "react";
import { FrontendExtension } from "../../types/extension";
import { SLOTS } from "../../types/slots";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { TerminalInstance } from "./UnifiedTerminal";
import { useKeybindingManager } from "../../kernel/KeybindingManagerContext";

/**
 * TerminalPanel — Single visible container that swaps terminal elements on tab switch.
 *
 * Instead of N absolutely-positioned containers with visibility toggling (which
 * causes WebGL context loss and blank screens), this uses one always-visible
 * container. On tab switch, the active terminal's DOM element is reparented into
 * it. Inactive terminals keep buffering PTY output in memory and redraw
 * instantly on reattach.
 */
function TerminalPanel() {
  const containerRef = useRef<HTMLDivElement>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [sessions, setSessions] = useState<Set<string>>(new Set());
  const instancesRef = useRef(new Map<string, TerminalInstance>());
  const activeIdRef = useRef<string | null>(null);
  const keybindingManager = useKeybindingManager();
  const mountedRef = useRef(true);

  // Keep ref in sync for non-React callbacks (ResizeObserver)
  activeIdRef.current = activeId;

  // Listen for session lifecycle events
  useEffect(() => {
    mountedRef.current = true;
    const unsubs: Array<() => void> = [];

    const setup = async () => {
      const u = await listen<{
        topic: string;
        payload: Record<string, unknown>;
      }>("core-event", (e) => {
        if (!mountedRef.current) return;
        const { topic, payload } = e.payload;
        const sid = payload.session_id as string;

        switch (topic) {
          case "session.created":
            setSessions((prev) => new Set(prev).add(sid));
            setActiveId(sid);
            break;

          case "session.closed": {
            const inst = instancesRef.current.get(sid);
            if (inst) {
              inst.dispose();
              instancesRef.current.delete(sid);
            }
            setSessions((prev) => {
              const next = new Set(prev);
              next.delete(sid);
              return next;
            });
            setActiveId((cur) => (cur === sid ? null : cur));
            break;
          }

          case "session.active_changed":
            if (sid) {
              setActiveId(sid);
              setSessions((prev) =>
                prev.has(sid) ? prev : new Set(prev).add(sid),
              );
            }
            break;
        }
      });
      if (!mountedRef.current) {
        u();
        return;
      }
      unsubs.push(u);
    };

    setup();

    // Load initial active session
    invoke<string | null>("get_active_session").then((active) => {
      if (mountedRef.current && active) {
        setActiveId(active);
        setSessions((prev) =>
          prev.has(active) ? prev : new Set(prev).add(active),
        );
      }
    });

    return () => {
      mountedRef.current = false;
      unsubs.forEach((u) => u());
      instancesRef.current.forEach((inst) => inst.dispose());
      instancesRef.current.clear();
    };
  }, []);

  // Ensure a TerminalInstance exists for every known session
  useEffect(() => {
    for (const sid of sessions) {
      if (!instancesRef.current.has(sid)) {
        instancesRef.current.set(
          sid,
          new TerminalInstance(sid, keybindingManager),
        );
      }
    }
  }, [sessions, keybindingManager]);

  // Activate the current terminal — clear container and reparent its element
  useEffect(() => {
    if (!containerRef.current) return;
    const container = containerRef.current;

    // Clear previous content (old terminal element or placeholder)
    while (container.firstChild) {
      container.removeChild(container.firstChild);
    }

    if (!activeId) {
      // Show placeholder
      const placeholder = document.createElement("div");
      placeholder.style.cssText =
        "width:100%;height:100%;display:flex;align-items:center;justify-content:center;" +
        "background:var(--terminal-bg,#1e1e1e);color:var(--text-tertiary,#666);";
      placeholder.textContent = "No active session";
      container.appendChild(placeholder);
      return;
    }

    // Get or lazily create instance
    let instance = instancesRef.current.get(activeId);
    if (!instance) {
      instance = new TerminalInstance(activeId, keybindingManager);
      instancesRef.current.set(activeId, instance);
    }

    instance.activate(container);
  }, [activeId, keybindingManager]);

  // Single ResizeObserver on the container — only resizes the active terminal
  useEffect(() => {
    if (!containerRef.current) return;

    const observer = new ResizeObserver((entries) => {
      const { width, height } = entries[0].contentRect;
      if (width === 0 || height === 0) return;

      const id = activeIdRef.current;
      if (id) {
        instancesRef.current.get(id)?.fit();
      }
    });

    observer.observe(containerRef.current);
    return () => observer.disconnect();
  }, []);

  // Emit activity event for inactivity tracking
  const handleActivity = useCallback(() => {
    window.dispatchEvent(new CustomEvent("terminal:activity"));
  }, []);

  return (
    <div
      ref={containerRef}
      onKeyDown={handleActivity}
      onClick={handleActivity}
      style={{
        width: "100%",
        height: "100%",
        position: "relative",
        overflow: "hidden",
      }}
    />
  );
}

export function createTerminalPanelExtension(): FrontendExtension {
  return {
    manifest: {
      id: "terminal-panel",
      name: "Terminal Panel",
      version: "0.4.0",
      description: "Terminal panel with session management",
      dependencies: ["tab-bar"],
    },
    activate(ctx) {
      ctx.componentRegistry.register(SLOTS.MAIN_CONTENT, {
        id: "terminal-panel-main",
        component: TerminalPanel,
        priority: 10,
        extensionId: "terminal-panel",
      });
    },
  };
}
