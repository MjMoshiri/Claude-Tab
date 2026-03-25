import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { SessionInfo } from "../types/session";

export function useSession() {
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const list = await invoke<SessionInfo[]>("list_sessions");
    setSessions(list);
    const active = await invoke<string | null>("get_active_session");
    setActiveId(active);
  }, []);

  useEffect(() => {
    let mounted = true;
    refresh();

    let unlisten: (() => void) | null = null;
    listen<{ topic: string; payload: Record<string, unknown> }>("core-event", (e) => {
      if (!mounted) return;
      const { topic } = e.payload;
      if (
        topic === "session.created" ||
        topic === "session.closed" ||
        topic === "session.state_changed" ||
        topic === "session.renamed" ||
        topic === "session.active_changed" ||
        topic === "session.metadata_changed"
      ) {
        refresh();
      }
    }).then((u) => {
      if (!mounted) { u(); return; }
      unlisten = u;
    });

    return () => {
      mounted = false;
      if (unlisten) unlisten();
    };
  }, [refresh]);

  return { sessions, activeId, refresh };
}
