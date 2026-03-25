import { useState, useEffect, useRef, useCallback } from "react";
import { FrontendExtension } from "../../types/extension";
import { SLOTS } from "../../types/slots";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useConfig } from "../../kernel/ConfigProvider";

type PolicyMode = "off" | "on" | "allow-all";

function deriveMode(policy: string | null): PolicyMode {
  if (!policy || policy.length === 0) return "off";
  if (policy === "*") return "allow-all";
  return "on";
}

/**
 * PolicyBadge — Tiny indicator in TAB_BAR_LEFT showing the active session's
 * auto-accept policy. Click to view/edit; changes take effect mid-session
 * via file-based policy (no restart needed).
 */
function PolicyBadge() {
  const [activeId, setActiveId] = useState<string | null>(null);
  const [policy, setPolicy] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState("");
  const [mode, setMode] = useState<PolicyMode>("off");
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);
  const config = useConfig();
  const autoAcceptEnabled = config.get<boolean>("autoAccept.enabled", false);

  // Track active session
  useEffect(() => {
    let mounted = true;
    invoke<string | null>("get_active_session").then((id) => {
      if (mounted) setActiveId(id);
    });
    const unsubs: Array<() => void> = [];
    listen<{ topic: string; payload: Record<string, unknown> }>(
      "core-event",
      (e) => {
        if (!mounted) return;
        const { topic, payload } = e.payload;
        if (topic === "session.active_changed" || topic === "session.created") {
          setActiveId(payload.session_id as string);
        }
        if (topic === "session.closed") {
          setActiveId((cur) =>
            cur === (payload.session_id as string) ? null : cur,
          );
        }
      },
    ).then((u) => {
      if (!mounted) { u(); return; }
      unsubs.push(u);
    });
    return () => {
      mounted = false;
      unsubs.forEach((u) => u());
    };
  }, []);

  // Load policy when active session changes
  useEffect(() => {
    if (!activeId) {
      setPolicy(null);
      return;
    }
    let mounted = true;
    invoke<string | null>("get_session_policy", {
      sessionId: activeId,
    }).then((p) => {
      if (!mounted) return;
      setPolicy(p);
      setMode(deriveMode(p));
    });
    return () => { mounted = false; };
  }, [activeId]);

  // Close popover on outside click
  useEffect(() => {
    if (!editing) return;
    const handler = (e: MouseEvent) => {
      if (
        popoverRef.current &&
        !popoverRef.current.contains(e.target as Node)
      ) {
        setEditing(false);
      }
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [editing]);

  // Focus textarea on open
  useEffect(() => {
    if (editing && mode === "on" && textareaRef.current) {
      textareaRef.current.focus();
      textareaRef.current.selectionStart = textareaRef.current.value.length;
    }
  }, [editing, mode]);

  const handleOpen = useCallback(() => {
    const m = deriveMode(policy);
    setMode(m);
    setDraft(m === "on" ? (policy ?? "") : "");
    setEditing(true);
  }, [policy]);

  const applyPolicy = useCallback(async (newPolicy: string) => {
    if (!activeId) return;
    await invoke("set_session_policy", {
      sessionId: activeId,
      policy: newPolicy,
    });
    setPolicy(newPolicy || null);
    setMode(deriveMode(newPolicy || null));
    setEditing(false);
  }, [activeId]);

  const handleSave = useCallback(() => {
    if (mode === "off") applyPolicy("");
    else if (mode === "allow-all") applyPolicy("*");
    else applyPolicy(draft);
  }, [mode, draft, applyPolicy]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && e.metaKey) {
        e.preventDefault();
        handleSave();
      }
      if (e.key === "Escape") {
        setEditing(false);
      }
    },
    [handleSave],
  );

  if (!activeId || !autoAcceptEnabled) return null;

  const currentMode = deriveMode(policy);
  const badgeColor =
    currentMode === "allow-all"
      ? "var(--orange, #FF9F0A)"
      : currentMode === "on"
        ? "var(--green, #30D158)"
        : "var(--text-tertiary, #666)";
  const badgeLabel =
    currentMode === "allow-all"
      ? "Allow All"
      : currentMode === "on"
        ? "Policy"
        : "Off";

  return (
    <div style={{ position: "relative", marginRight: 6 }}>
      <button
        onClick={handleOpen}
        title={
          currentMode === "allow-all"
            ? "Auto-accept: allowing all"
            : currentMode === "on"
              ? `Policy: ${policy}`
              : "Auto-accept off"
        }
        style={{
          background: "none",
          border: `1px solid ${badgeColor}`,
          borderRadius: 4,
          color: badgeColor,
          fontSize: 10,
          fontWeight: 600,
          padding: "2px 6px",
          cursor: "pointer",
          whiteSpace: "nowrap",
          lineHeight: "16px",
          opacity: currentMode !== "off" ? 1 : 0.6,
        }}
      >
        {badgeLabel}
      </button>

      {editing && (
        <div
          ref={popoverRef}
          style={{
            position: "absolute",
            top: "calc(100% + 4px)",
            left: 0,
            zIndex: 1000,
            background: "var(--bg-elevated, #2a2a2a)",
            border: "1px solid var(--border-subtle, #444)",
            borderRadius: 8,
            padding: 12,
            width: 320,
            boxShadow: "0 8px 24px rgba(0,0,0,0.4)",
          }}
        >
          <div
            style={{
              fontSize: 11,
              fontWeight: 600,
              color: "var(--text-secondary, #aaa)",
              marginBottom: 8,
            }}
          >
            Auto-Accept Mode
          </div>

          {/* Mode selector */}
          <div style={{ display: "flex", gap: 4, marginBottom: 8 }}>
            {(["off", "on", "allow-all"] as PolicyMode[]).map((m) => (
              <button
                key={m}
                onClick={() => setMode(m)}
                style={{
                  flex: 1,
                  background: mode === m ? "var(--accent, #0A84FF)" : "var(--bg-primary, #1e1e1e)",
                  color: mode === m ? "#fff" : "var(--text-secondary, #aaa)",
                  border: `1px solid ${mode === m ? "var(--accent, #0A84FF)" : "var(--border-subtle, #444)"}`,
                  borderRadius: 4,
                  padding: "4px 8px",
                  fontSize: 11,
                  fontWeight: 600,
                  cursor: "pointer",
                }}
              >
                {m === "off" ? "Off" : m === "on" ? "Policy" : "Allow All"}
              </button>
            ))}
          </div>

          {/* Policy textarea only shown in "on" mode */}
          {mode === "on" && (
            <textarea
              ref={textareaRef}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="e.g. Allow all edits and tests. Deny git push."
              rows={3}
              style={{
                width: "100%",
                background: "var(--bg-primary, #1e1e1e)",
                color: "var(--text-primary, #e5e5e5)",
                border: "1px solid var(--border-subtle, #444)",
                borderRadius: 4,
                padding: 8,
                fontSize: 12,
                fontFamily: "inherit",
                resize: "vertical",
                outline: "none",
                boxSizing: "border-box",
              }}
            />
          )}

          {mode === "allow-all" && (
            <div style={{ fontSize: 11, color: "var(--orange, #FF9F0A)", marginBottom: 4 }}>
              All tool calls will be auto-accepted without asking.
            </div>
          )}

          {mode === "off" && (
            <div style={{ fontSize: 11, color: "var(--text-tertiary, #666)", marginBottom: 4 }}>
              Normal permission dialogs will be shown.
            </div>
          )}

          <div
            style={{
              display: "flex",
              justifyContent: "flex-end",
              marginTop: 8,
              gap: 6,
            }}
          >
            <button
              onClick={() => setEditing(false)}
              style={{
                background: "none",
                border: "1px solid var(--border-subtle, #444)",
                color: "var(--text-secondary, #aaa)",
                borderRadius: 4,
                padding: "4px 10px",
                fontSize: 11,
                cursor: "pointer",
              }}
            >
              Cancel
            </button>
            <button
              onClick={handleSave}
              style={{
                background: "var(--accent, #0A84FF)",
                border: "none",
                color: "#fff",
                borderRadius: 4,
                padding: "4px 10px",
                fontSize: 11,
                cursor: "pointer",
                fontWeight: 600,
              }}
            >
              Save
            </button>
          </div>
          <div
            style={{
              fontSize: 10,
              color: "var(--text-tertiary, #666)",
              marginTop: 6,
            }}
          >
            Changes apply immediately to this session.
          </div>
        </div>
      )}
    </div>
  );
}

export function createPolicyBadgeExtension(): FrontendExtension {
  return {
    manifest: {
      id: "policy-badge",
      name: "Policy Badge",
      version: "0.2.0",
      description: "Per-session auto-accept policy editor",
    },
    activate(ctx) {
      ctx.componentRegistry.register(SLOTS.TAB_BAR_LEFT, {
        id: "policy-badge",
        component: PolicyBadge,
        priority: 5,
        extensionId: "policy-badge",
      });
    },
  };
}
