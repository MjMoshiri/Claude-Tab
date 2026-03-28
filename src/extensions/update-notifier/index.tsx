import { useState, useEffect, useCallback, useRef } from "react";
import { FrontendExtension } from "../../types/extension";
import { SLOTS } from "../../types/slots";
import { useConfig } from "../../kernel/ConfigProvider";

const CHECK_INTERVAL_MS = 5 * 60 * 1000;

function UpdateNotifier() {
  const config = useConfig();
  const autoCheck = config.get<boolean>("update.autoCheck", true);
  const [state, setState] = useState<"idle" | "downloading" | "ready">("idle");
  const [version, setVersion] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const busyRef = useRef(false);

  const doCheck = useCallback(async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    try {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      if (update) {
        setVersion(update.version);
        setDismissed(false);
        setState("downloading");
        await update.downloadAndInstall();
        setState("ready");
      }
    } catch {
      // Silently fail — next interval will retry
    } finally {
      busyRef.current = false;
    }
  }, []);

  useEffect(() => {
    if (!autoCheck || state === "ready") return;
    doCheck();
    const id = setInterval(doCheck, CHECK_INTERVAL_MS);
    return () => clearInterval(id);
  }, [autoCheck, state, doCheck]);

  if (state === "downloading") {
    return (
      <span className="status-item" style={{ color: "var(--text-tertiary, #666)", fontSize: 11 }}>
        Updating...
      </span>
    );
  }

  if (state !== "ready" || dismissed) return null;

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 4 }}>
      <button
        onClick={async () => {
          const { relaunch } = await import("@tauri-apps/plugin-process");
          await relaunch();
        }}
        title={`v${version} downloaded — click to restart`}
        style={{
          background: "none",
          border: "1px solid var(--green, #30D158)",
          borderRadius: 4,
          color: "var(--green, #30D158)",
          fontSize: 10,
          fontWeight: 600,
          padding: "2px 6px",
          cursor: "pointer",
          lineHeight: "16px",
          whiteSpace: "nowrap",
        }}
      >
        v{version} — Restart
      </button>
      <button
        onClick={() => setDismissed(true)}
        title="Dismiss — applies on next restart"
        style={{
          background: "none",
          border: "none",
          color: "var(--text-tertiary, #666)",
          fontSize: 12,
          cursor: "pointer",
          padding: "0 2px",
          lineHeight: "16px",
        }}
      >
        ×
      </button>
    </div>
  );
}

export function createUpdateNotifierExtension(): FrontendExtension {
  return {
    manifest: {
      id: "update-notifier",
      name: "Update Notifier",
      version: "0.1.0",
      description: "Auto-check for updates and show status bar indicator",
    },
    activate(ctx) {
      ctx.componentRegistry.register(SLOTS.STATUS_BAR_RIGHT, {
        id: "update-notifier",
        component: UpdateNotifier,
        priority: 100,
        extensionId: "update-notifier",
      });
    },
  };
}
