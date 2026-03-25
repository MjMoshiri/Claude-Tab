import React, { useState, useEffect, useCallback, useRef } from "react";
import { FrontendExtension } from "../../types/extension";
import { SessionInfo, WorktreeInfo } from "../../types/session";
import { SLOTS } from "../../types/slots";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, confirm } from "@tauri-apps/plugin-dialog";
import { SearchBar } from "./SearchBar";
import { HistorySection } from "./HistorySection";
import { ClaudeSession } from "./types";
import { toggleSettings } from "../settings";
import { toggleProfiles } from "../profiles";
import { SkillPicker } from "../profiles/SkillPicker";
import { SystemPromptPicker } from "../profiles/SystemPromptPicker";

const MIN_WIDTH = 150;
const MAX_WIDTH = 400;

// Custom event for toggling sidebar from keybinding
const SIDEBAR_TOGGLE_EVENT = "tab-bar:toggle-sidebar";

async function cycleTabs(direction: 1 | -1, stateFilter?: string) {
  const sessions = await invoke<SessionInfo[]>("list_sessions");
  const active = await invoke<string | null>("get_active_session");
  const filtered = stateFilter
    ? sessions.filter((s) => s.state === stateFilter)
    : sessions;
  if (filtered.length === 0) return;
  const idx = filtered.findIndex((s) => s.id === active);
  const nextIdx =
    idx === -1 ? 0 : (idx + direction + filtered.length) % filtered.length;
  invoke("set_active_session", { sessionId: filtered[nextIdx].id });
}

const STATE_COLORS: Record<string, string> = {
  active: "var(--green, #30D158)",
  running: "var(--accent, #0A84FF)",
  your_turn: "var(--orange, #FF9F0A)",
  completed: "var(--yellow, #FFD60A)",
  paused: "var(--red, #FF453A)",
  idle: "var(--text-tertiary, #666)",
};

const STATE_LABELS: Record<string, string> = {
  active: "Active",
  running: "Running",
  your_turn: "Your Turn",
  completed: "Completed",
  paused: "Paused",
  idle: "Idle",
};

function ContextMenu({
  x,
  y,
  sessionId,
  sessionState,
  previousSessionId,
  onClose,
  onRefresh,
  onRename,
  onSetPolicy,
}: {
  x: number;
  y: number;
  sessionId: string;
  sessionState: string;
  previousSessionId: string | null;
  onClose: () => void;
  onRefresh: () => void;
  onRename: (id: string) => void;
  onSetPolicy: (id: string) => void;
}) {
  const menuRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });

  // Reposition if overflowing viewport
  useEffect(() => {
    if (menuRef.current) {
      const rect = menuRef.current.getBoundingClientRect();
      let newX = x;
      let newY = y;
      if (rect.right > window.innerWidth) {
        newX = window.innerWidth - rect.width - 8;
      }
      if (rect.bottom > window.innerHeight) {
        newY = window.innerHeight - rect.height - 8;
      }
      if (newX !== x || newY !== y) {
        setPos({ x: Math.max(8, newX), y: Math.max(8, newY) });
      }
    }
  }, [x, y]);

  useEffect(() => {
    const handler = () => onClose();
    document.addEventListener("click", handler);
    return () => document.removeEventListener("click", handler);
  }, [onClose]);

  const handleRename = () => {
    onRename(sessionId);
    onClose();
  };

  const handleSetPolicy = () => {
    onSetPolicy(sessionId);
    onClose();
  };

  const handleFork = async () => {
    try {
      await invoke("fork_active_session", { sessionId });
      onRefresh();
    } catch (err) {
      console.error("[ContextMenu] Fork failed:", err);
    }
    onClose();
  };

  const handleMarkIdle = async () => {
    try {
      await invoke("set_session_state", { sessionId, newState: "idle" });
      onRefresh();
    } catch (err) {
      console.error("[ContextMenu] Mark as idle failed:", err);
    }
    onClose();
  };

  const handleMarkActive = async () => {
    try {
      await invoke("set_session_state", { sessionId, newState: "active" });
      onRefresh();
    } catch (err) {
      console.error("[ContextMenu] Mark as active failed:", err);
    }
    onClose();
  };

  const handleHide = async () => {
    try {
      await invoke("set_session_hidden", { sessionId, hidden: true });
      onRefresh();
    } catch (err) {
      console.error("[ContextMenu] Hide failed:", err);
    }
    onClose();
  };

  const handleViewChain = async () => {
    try {
      const chain = await invoke<SessionInfo[]>("get_session_chain", { sessionId });
      console.log("[ContextMenu] Session chain:", chain.map((s) => `${s.title} (${s.id.slice(0, 8)})`));
    } catch (err) {
      console.error("[ContextMenu] View chain failed:", err);
    }
    onClose();
  };

  const handleClose = async () => {
    try {
      await invoke("close_session", { sessionId });
      onRefresh();
    } catch (err) {
      console.error("[ContextMenu] Close failed:", err);
    }
    onClose();
  };

  return (
    <div
      ref={menuRef}
      className="history-context-menu"
      role="menu"
      aria-label="Session actions"
      style={{ top: pos.y, left: pos.x }}
    >
      <button type="button" role="menuitem" className="history-context-item" onClick={handleRename}>
        Rename
      </button>
      <button type="button" role="menuitem" className="history-context-item" onClick={handleFork}>
        Fork
      </button>
      {sessionState !== "idle" && (
        <button type="button" role="menuitem" className="history-context-item" onClick={handleMarkIdle}>
          Mark as Idle
        </button>
      )}
      {sessionState === "idle" && (
        <button type="button" role="menuitem" className="history-context-item" onClick={handleMarkActive}>
          Mark as Active
        </button>
      )}
      <button type="button" role="menuitem" className="history-context-item" onClick={handleHide}>
        Hide Session
      </button>
      {previousSessionId && (
        <button type="button" role="menuitem" className="history-context-item" onClick={handleViewChain}>
          View History Chain
        </button>
      )}
      <button type="button" role="menuitem" className="history-context-item" onClick={handleSetPolicy}>
        Set Policy
      </button>
      <button type="button" role="menuitem" className="history-context-item" onClick={handleClose}>
        Close
      </button>
    </div>
  );
}

function PolicyPopover({ sessionId, x, y, onClose }: { sessionId: string; x: number; y: number; onClose: () => void }) {
  const [draft, setDraft] = useState("");
  const [loading, setLoading] = useState(true);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    invoke<string | null>("get_session_policy", { sessionId }).then((p) => {
      setDraft(p ?? "");
      setLoading(false);
    });
  }, [sessionId]);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener("mousedown", handler);
    return () => document.removeEventListener("mousedown", handler);
  }, [onClose]);

  const handleSave = async () => {
    await invoke("set_session_policy", { sessionId, policy: draft });
    onClose();
  };

  return (
    <div
      ref={ref}
      className="history-context-menu"
      style={{ top: y, left: x, padding: 12, width: 280 }}
    >
      <div style={{ fontSize: 11, fontWeight: 600, color: "var(--text-secondary, #aaa)", marginBottom: 6 }}>
        Session Policy
      </div>
      {loading ? (
        <div style={{ fontSize: 11, color: "var(--text-tertiary, #666)" }}>Loading...</div>
      ) : (
        <>
          <textarea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && e.metaKey) { e.preventDefault(); handleSave(); }
              if (e.key === "Escape") onClose();
            }}
            placeholder="e.g. Allow all edits and tests. Deny git push."
            rows={3}
            autoFocus
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
          <div style={{ display: "flex", justifyContent: "flex-end", marginTop: 8, gap: 6 }}>
            <button
              onClick={onClose}
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
        </>
      )}
    </div>
  );
}

function ResizeHandle({ onResize }: { onResize: (delta: number) => void }) {
  const isDragging = useRef(false);
  const startX = useRef(0);

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (!isDragging.current) return;
      const delta = e.clientX - startX.current;
      startX.current = e.clientX;
      onResize(delta);
    };
    const handleMouseUp = () => {
      isDragging.current = false;
      document.body.style.cursor = '';
      document.body.style.userSelect = '';
    };
    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
    return () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
  }, [onResize]);

  return (
    <div
      className="sidebar-resize-handle"
      onMouseDown={(e) => {
        isDragging.current = true;
        startX.current = e.clientX;
        document.body.style.cursor = 'col-resize';
        document.body.style.userSelect = 'none';
      }}
    />
  );
}

function SidePanel() {
  const [sessions, setSessions] = useState<SessionInfo[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editValue, setEditValue] = useState("");
  const searchInputRef = useRef<HTMLInputElement | null>(null);
  const sidebarRef = useRef<HTMLElement | null>(null);
  const [sidebarVisible, setSidebarVisible] = useState(
    () => localStorage.getItem("sidebarVisible") !== "false"
  );
  const [width, setWidth] = useState(
    () => parseInt(localStorage.getItem("sidebarWidth") || "200", 10)
  );
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    sessionId: string;
    sessionState: string;
    previousSessionId: string | null;
  } | null>(null);
  const [policyEditor, setPolicyEditor] = useState<{sessionId: string; x: number; y: number} | null>(null);

  // Apply sidebar width/visibility to the parent .app-sidebar element
  useEffect(() => {
    // Find the parent sidebar element via DOM traversal
    const el = sidebarRef.current?.closest(".app-sidebar") as HTMLElement;
    if (!el) return;

    if (sidebarVisible) {
      el.classList.remove("collapsed");
      el.style.width = `${width}px`;
      el.style.minWidth = `${width}px`;
    } else {
      el.classList.add("collapsed");
      el.style.width = "";
      el.style.minWidth = "";
    }
  }, [sidebarVisible, width]);

  // Listen for toggle sidebar events from keybindings
  useEffect(() => {
    const handleToggle = () => {
      setSidebarVisible(prev => {
        const next = !prev;
        localStorage.setItem("sidebarVisible", String(next));
        return next;
      });
    };
    window.addEventListener(SIDEBAR_TOGGLE_EVENT, handleToggle);
    return () => window.removeEventListener(SIDEBAR_TOGGLE_EVENT, handleToggle);
  }, []);

  const handleResize = useCallback((delta: number) => {
    setWidth(prev => {
      const newWidth = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, prev + delta));
      localStorage.setItem("sidebarWidth", String(newWidth));
      return newWidth;
    });
  }, []);

  const refresh = useCallback(async () => {
    try {
      const list = await invoke<SessionInfo[]>("list_sessions");
      setSessions(list);
      const active = await invoke<string | null>("get_active_session");
      setActiveId(active);
    } catch (err) {
      console.error("[SidePanel] Failed to refresh:", err);
    }
  }, []);

  useEffect(() => {
    let mounted = true;
    refresh();
    const unsubs: Array<() => void> = [];

    listen<{ topic: string; payload: Record<string, unknown> }>("core-event", (e) => {
      if (!mounted) return;
      const { topic, payload } = e.payload;

      if (topic === "session.active_changed") {
        const newActiveId = payload.session_id as string;
        if (newActiveId) {
          setActiveId(newActiveId);
        }
        return;
      }

      if (
        topic === "session.created" ||
        topic === "session.closed" ||
        topic === "session.state_changed" ||
        topic === "session.renamed" ||
        topic === "session.metadata_changed"
      ) {
        refresh();
      }
    }).then((u) => {
      if (!mounted) { u(); return; }
      unsubs.push(u);
    });

    return () => { mounted = false; unsubs.forEach((u) => u()); };
  }, [refresh]);

  const handleSelect = async (id: string) => {
    await invoke("set_active_session", { sessionId: id });
    setActiveId(id);
  };

  const handleClose = async (id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    await invoke("close_session", { sessionId: id });
    refresh();
  };

  const [showTitlePrompt, setShowTitlePrompt] = useState(false);
  const [pendingDir, setPendingDir] = useState<string | null>(null);
  const [titleInput, setTitleInput] = useState("");
  const [useWorktree, setUseWorktree] = useState(false);
  const [worktreeBranch, setWorktreeBranch] = useState("");
  const [isGitRepo, setIsGitRepo] = useState(false);
  const [creatingSession, setCreatingSession] = useState(false);
  const [selectedSkills, setSelectedSkills] = useState<Set<string> | null>(null);
  const [systemPromptFile, setSystemPromptFile] = useState<string | null>(null);

  const handleNewTab = async () => {
    const dir = await open({
      title: "Select Working Directory",
      directory: true,
    });
    if (!dir) return;
    const dirName = dir.split("/").filter(Boolean).pop() || dir;
    setPendingDir(dir);
    setTitleInput(dirName);
    setUseWorktree(false);
    setWorktreeBranch("");
    setSelectedSkills(null);
    setSystemPromptFile(null);

    // Load all skills and default to all selected
    invoke<{ name: string }[]>("list_available_skills")
      .then((skills) => setSelectedSkills(new Set(skills.map((s) => s.name))))
      .catch(() => setSelectedSkills(new Set()));

    // Check if directory is a git repo
    try {
      const gitRepo = await invoke<boolean>("check_git_repo", { path: dir });
      setIsGitRepo(gitRepo);
    } catch {
      setIsGitRepo(false);
    }

    setShowTitlePrompt(true);
  };

  // Listen for new-tab trigger from Cmd+T keybinding
  useEffect(() => {
    const handler = () => { handleNewTab(); };
    window.addEventListener("tab-bar:new-tab-trigger", handler);
    return () => window.removeEventListener("tab-bar:new-tab-trigger", handler);
  }, []);

  const handleTitleSubmit = async () => {
    if (!pendingDir || creatingSession) return;
    setCreatingSession(true);

    try {
      const title = titleInput.trim() || pendingDir.split("/").filter(Boolean).pop() || pendingDir;
      let workingDir = pendingDir;
      let metadata: Record<string, unknown> | undefined;

      if (useWorktree && isGitRepo) {
        const branch = worktreeBranch.trim() || undefined;
        const wt = await invoke<WorktreeInfo>("create_worktree", {
          repoPath: pendingDir,
          branchName: branch,
        });
        workingDir = wt.path;
        metadata = {
          worktree_path: wt.path,
          worktree_branch: wt.branch,
          worktree_repo: wt.repo_path,
        };
      }

      // Sync selected skills before creating session
      if (selectedSkills && selectedSkills.size > 0) {
        await invoke("sync_skills", { skills: [...selectedSkills] });
      }

      await invoke("create_session", {
        request: {
          provider_id: "claude-code",
          title,
          working_directory: workingDir,
          system_prompt_file: systemPromptFile || undefined,
          metadata,
        },
      });

      setShowTitlePrompt(false);
      setPendingDir(null);
      setTitleInput("");
      setUseWorktree(false);
      setWorktreeBranch("");
      setIsGitRepo(false);
      setSelectedSkills(null);
      setSystemPromptFile(null);
    } catch (err) {
      console.error("[SidePanel] Session creation failed:", err);
    } finally {
      setCreatingSession(false);
    }
  };

  const handleTitleCancel = () => {
    setShowTitlePrompt(false);
    setPendingDir(null);
    setTitleInput("");
    setUseWorktree(false);
    setWorktreeBranch("");
    setIsGitRepo(false);
    setSelectedSkills(null);
    setSystemPromptFile(null);
  };

  // Worktree cleanup listener
  useEffect(() => {
    let mounted = true;
    const unsubs: Array<() => void> = [];

    listen<{ topic: string; payload: Record<string, unknown> }>("core-event", async (e) => {
      if (!mounted) return;
      const { topic, payload } = e.payload;
      if (topic !== "session.worktree_cleanup") return;

      const wtPath = payload.worktree_path as string;
      const wtBranch = payload.worktree_branch as string;

      const shouldRemove = await confirm(
        `Remove worktree "${wtBranch}"?\n\nPath: ${wtPath}`,
        { title: "Worktree Cleanup", kind: "warning" }
      );

      if (shouldRemove) {
        try {
          await invoke("remove_worktree", { worktreePath: wtPath });
        } catch (err) {
          console.error("[SidePanel] Worktree removal failed:", err);
        }
      }
    }).then((u) => {
      if (!mounted) { u(); return; }
      unsubs.push(u);
    });

    return () => { mounted = false; unsubs.forEach((u) => u()); };
  }, []);

  const handleNewTerminal = async () => {
    await invoke("create_session", {
      request: { provider_id: "terminal", title: "Terminal" },
    });
  };

  // Listen for F2 rename trigger
  useEffect(() => {
    const handleRenameEvent = () => {
      if (activeId) {
        const s = sessions.find((s) => s.id === activeId);
        setEditingId(activeId);
        setEditValue(s?.title || "");
      }
    };
    window.addEventListener("tab-bar:rename-active", handleRenameEvent);
    return () => window.removeEventListener("tab-bar:rename-active", handleRenameEvent);
  }, [activeId, sessions]);

  const handleSearchResultClick = async (result: { session_id: string }) => {
    try {
      await invoke("resume_session", { claudeSessionId: result.session_id });
      refresh();
    } catch (err) {
      console.error("[SidePanel] Resume from search failed:", err);
    }
  };

  const handleResume = async (session: ClaudeSession) => {
    try {
      await invoke("resume_session", { claudeSessionId: session.session_id });
      refresh();
    } catch (err) {
      console.error("[SidePanel] Resume failed:", err);
    }
  };

  const handleFork = async (session: ClaudeSession) => {
    try {
      await invoke("fork_session", { claudeSessionId: session.session_id });
      refresh();
    } catch (err) {
      console.error("[SidePanel] Fork failed:", err);
    }
  };

  const handleContextMenu = (e: React.MouseEvent, session: SessionInfo) => {
    e.preventDefault();
    setContextMenu({
      x: e.clientX,
      y: e.clientY,
      sessionId: session.id,
      sessionState: session.state,
      previousSessionId: session.previous_session_id,
    });
  };

  const handleDoubleClick = (s: SessionInfo) => {
    setEditingId(s.id);
    setEditValue(s.title || "");
  };

  const handleRenameSubmit = async (id: string) => {
    const trimmed = editValue.trim();
    if (trimmed) {
      await invoke("rename_session", { sessionId: id, title: trimmed });
    }
    setEditingId(null);
  };

  const handleRenameKeyDown = (e: React.KeyboardEvent, id: string) => {
    if (e.key === "Enter") {
      handleRenameSubmit(id);
    } else if (e.key === "Escape") {
      setEditingId(null);
    }
  };

  const startRename = (id: string) => {
    const s = sessions.find((s) => s.id === id);
    setEditingId(id);
    setEditValue(s?.title || "");
  };

  // Separate terminal sessions from Claude sessions
  const claudeSessions = sessions.filter((s) => s.provider_id !== "terminal");
  const terminalSessions = sessions.filter((s) => s.provider_id === "terminal");

  // Group Claude sessions by state
  const groups = claudeSessions.reduce<Record<string, SessionInfo[]>>((acc, s) => {
    const key = s.state;
    if (!acc[key]) acc[key] = [];
    acc[key].push(s);
    return acc;
  }, {});

  const groupOrder = ["your_turn", "completed", "running", "active", "paused", "idle"];
  const sortedGroups = Object.entries(groups).sort(([a], [b]) => {
    const ai = groupOrder.indexOf(a);
    const bi = groupOrder.indexOf(b);
    return (ai === -1 ? 99 : ai) - (bi === -1 ? 99 : bi);
  });

  const toggleGroup = (state: string) => {
    setCollapsed((prev) => ({ ...prev, [state]: !prev[state] }));
  };

  const renderSessionItem = (s: SessionInfo) => (
    <div
      key={s.id}
      className={`side-panel-item ${s.id === activeId ? "side-panel-item-active" : ""}`}
      onClick={() => handleSelect(s.id)}
      onContextMenu={(e) => handleContextMenu(e, s)}
      title={s.summary ?? s.working_directory ?? undefined}
      role="treeitem"
      aria-selected={s.id === activeId}
    >
      <div className="side-panel-item-info">
        {editingId === s.id ? (
          <input
            className="side-panel-rename-input"
            value={editValue}
            onChange={(e) => setEditValue(e.target.value)}
            onBlur={() => handleRenameSubmit(s.id)}
            onKeyDown={(e) => handleRenameKeyDown(e, s.id)}
            autoFocus
            onClick={(e) => e.stopPropagation()}
            aria-label="Rename session"
          />
        ) : (
          <span className="side-panel-item-title" onDoubleClick={() => handleDoubleClick(s)}>
            {s.title || `Session ${s.id.slice(0, 6)}`}
          </span>
        )}
        {s.subtitle && (
          <span className="side-panel-item-subtitle">{s.subtitle}</span>
        )}
        {s.tags && s.tags.length > 0 && (
          <div className="side-panel-item-tags">
            {s.tags.map((tag) => (
              <span key={tag} className="side-panel-tag">{tag}</span>
            ))}
          </div>
        )}
        {s.working_directory && (
          <span className="side-panel-item-dir">
            {s.working_directory.split("/").filter(Boolean).pop()}
          </span>
        )}
      </div>
      <button
        className="side-panel-item-close"
        onClick={(e) => handleClose(s.id, e)}
        aria-label={`Close ${s.title || 'session'}`}
      >
        &times;
      </button>
    </div>
  );

  return (
    <div className="side-panel" ref={el => { sidebarRef.current = el; }} role="tree" aria-label="Session list">
      <div className="side-panel-toolbar" role="toolbar" aria-label="Session actions">
        <SearchBar onResultClick={handleSearchResultClick} inputRef={searchInputRef} />
        <button className="side-panel-new-btn" onClick={handleNewTab} title="New Session (Cmd+T)" aria-label="New Session">
          +
        </button>
        <button className="side-panel-new-btn" onClick={handleNewTerminal} title="New Terminal (Cmd+Shift+T)" aria-label="New Terminal">
          $
        </button>
        <button className="side-panel-new-btn" onClick={toggleProfiles} title="Profiles (Cmd+Shift+P)" aria-label="Profiles">
          &#9654;
        </button>
        <button className="side-panel-new-btn" onClick={toggleSettings} title="Settings (Cmd+,)" aria-label="Settings">
          &#9881;
        </button>
      </div>
      <div className="side-panel-groups">
        {sortedGroups.map(([state, items]) => (
          <div key={state} className="side-panel-group" role="group" aria-label={`${STATE_LABELS[state] ?? state} sessions`}>
            <div
              className="side-panel-group-header"
              onClick={() => toggleGroup(state)}
              role="button"
              aria-expanded={!collapsed[state]}
            >
              <span
                className="side-panel-group-indicator"
                style={{ backgroundColor: STATE_COLORS[state] ?? "var(--text-tertiary)" }}
              />
              <span className="side-panel-group-label">
                {STATE_LABELS[state] ?? state}
              </span>
              <span className="side-panel-group-count">{items.length}</span>
              <span className={`side-panel-chevron ${collapsed[state] ? "collapsed" : ""}`}>
                &#9662;
              </span>
            </div>
            {!collapsed[state] && (
              <div className="side-panel-group-items">
                {items.map(renderSessionItem)}
              </div>
            )}
          </div>
        ))}
        {terminalSessions.length > 0 && (
          <div className="side-panel-group" role="group" aria-label="Terminal sessions">
            <div
              className="side-panel-group-header"
              onClick={() => toggleGroup("_terminals")}
              role="button"
              aria-expanded={!collapsed["_terminals"]}
            >
              <span
                className="side-panel-group-indicator"
                style={{ backgroundColor: "var(--text-secondary, #999)" }}
              />
              <span className="side-panel-group-label">Terminals</span>
              <span className="side-panel-group-count">{terminalSessions.length}</span>
              <span className={`side-panel-chevron ${collapsed["_terminals"] ? "collapsed" : ""}`}>
                &#9662;
              </span>
            </div>
            {!collapsed["_terminals"] && (
              <div className="side-panel-group-items">
                {terminalSessions.map(renderSessionItem)}
              </div>
            )}
          </div>
        )}
        {sessions.length === 0 && (
          <div className="side-panel-empty">
            <span>No active sessions</span>
            <span className="side-panel-empty-hint">
              Press <kbd className="kbd">&#8984;T</kbd> to start a new session
            </span>
          </div>
        )}
        <HistorySection onResume={handleResume} onFork={handleFork} />
      </div>
      {showTitlePrompt && (
        <div className="session-create-dialog">
          <input
            className="side-panel-rename-input"
            value={titleInput}
            onChange={(e) => setTitleInput(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) handleTitleSubmit();
              else if (e.key === "Escape") handleTitleCancel();
            }}
            placeholder="Session title..."
            autoFocus
            aria-label="New session title"
          />
          {isGitRepo && (
            <div className="session-create-worktree">
              <label className="session-create-checkbox">
                <input
                  type="checkbox"
                  checked={useWorktree}
                  onChange={(e) => setUseWorktree(e.target.checked)}
                />
                Use worktree
              </label>
              {useWorktree && (
                <input
                  className="side-panel-rename-input"
                  value={worktreeBranch}
                  onChange={(e) => setWorktreeBranch(e.target.value)}
                  placeholder="Branch name (auto-generated if empty)"
                  aria-label="Worktree branch name"
                />
              )}
            </div>
          )}
          {selectedSkills !== null && (
            <div className="session-create-skills">
              <label className="session-create-skills-label">Skills</label>
              <SkillPicker
                selectedSkills={selectedSkills}
                onSelectionChange={setSelectedSkills}
              />
            </div>
          )}
          <div className="session-create-system-prompt">
            <label className="session-create-system-prompt-label">System Prompt</label>
            <SystemPromptPicker
              selected={systemPromptFile}
              onSelect={setSystemPromptFile}
            />
          </div>
          <div className="session-create-actions">
            <button
              className="session-create-cancel"
              onClick={handleTitleCancel}
            >
              Cancel
            </button>
            <button
              className="session-create-submit"
              onClick={handleTitleSubmit}
              disabled={creatingSession}
            >
              {creatingSession ? "Creating..." : "Create"}
            </button>
          </div>
        </div>
      )}
      {contextMenu && (
        <ContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          sessionId={contextMenu.sessionId}
          sessionState={contextMenu.sessionState}
          previousSessionId={contextMenu.previousSessionId}
          onClose={() => setContextMenu(null)}
          onRefresh={refresh}
          onRename={startRename}
          onSetPolicy={(id) => { setPolicyEditor({ sessionId: id, x: contextMenu!.x, y: contextMenu!.y }); }}
        />
      )}
      {policyEditor && (
        <PolicyPopover
          sessionId={policyEditor.sessionId}
          x={policyEditor.x}
          y={policyEditor.y}
          onClose={() => setPolicyEditor(null)}
        />
      )}
      <ResizeHandle onResize={handleResize} />
    </div>
  );
}

const SET_TITLE_EVENT = "tab-bar:set-title";

function TitlePrompt() {
  const [visible, setVisible] = useState(false);
  const [value, setValue] = useState("");
  const [sessionId, setSessionId] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const show = async () => {
      const active = await invoke<string | null>("get_active_session");
      if (!active) return;
      const sessions = await invoke<SessionInfo[]>("list_sessions");
      const session = sessions.find((s) => s.id === active);
      setSessionId(active);
      setValue(session?.title || "");
      setVisible(true);
    };
    window.addEventListener(SET_TITLE_EVENT, show);
    return () => window.removeEventListener(SET_TITLE_EVENT, show);
  }, []);

  useEffect(() => {
    if (visible && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [visible]);

  const close = () => {
    setVisible(false);
    setValue("");
    setSessionId(null);
    const terminal = document.querySelector(".xterm-helper-textarea") as HTMLTextAreaElement;
    if (terminal) terminal.focus();
  };

  const submit = async () => {
    const trimmed = value.trim();
    if (trimmed && sessionId) {
      await invoke("rename_session", { sessionId, title: trimmed });
    }
    close();
  };

  if (!visible) return null;

  return (
    <div className="title-prompt-backdrop" onClick={close}>
      <div className="title-prompt" onClick={(e) => e.stopPropagation()}>
        <input
          ref={inputRef}
          className="title-prompt-input"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") submit();
            else if (e.key === "Escape") close();
          }}
          placeholder="Session title..."
          aria-label="Set session title"
        />
      </div>
    </div>
  );
}

function SidebarToggleButton() {
  const [visible, setVisible] = useState(
    () => localStorage.getItem("sidebarVisible") !== "false"
  );

  useEffect(() => {
    const handleToggle = () => {
      setVisible(prev => !prev);
    };
    window.addEventListener(SIDEBAR_TOGGLE_EVENT, handleToggle);
    return () => window.removeEventListener(SIDEBAR_TOGGLE_EVENT, handleToggle);
  }, []);

  return (
    <button
      className="sidebar-toggle"
      onClick={() => window.dispatchEvent(new Event(SIDEBAR_TOGGLE_EVENT))}
      title={visible ? "Hide Sidebar (Cmd+B)" : "Show Sidebar (Cmd+B)"}
      aria-label={visible ? "Hide sidebar" : "Show sidebar"}
      aria-expanded={visible}
    >
      {visible ? "\u2039" : "\u203A"}
    </button>
  );
}

export function createTabBarExtension(): FrontendExtension {
  return {
    manifest: {
      id: "tab-bar",
      name: "Tab Bar",
      version: "0.1.0",
      description: "Tab management UI with grouping",
    },
    activate(ctx) {
      ctx.componentRegistry.register(SLOTS.TAB_BAR_LEFT, {
        id: "tab-bar-sidebar-toggle",
        component: SidebarToggleButton,
        priority: 10,
        extensionId: "tab-bar",
      });

      ctx.componentRegistry.register(SLOTS.OVERLAY, {
        id: "tab-bar-title-prompt",
        component: TitlePrompt,
        priority: 20,
        extensionId: "tab-bar",
      });

      ctx.componentRegistry.register(SLOTS.SIDE_PANEL, {
        id: "tab-bar-side-panel",
        component: SidePanel,
        priority: 10,
        extensionId: "tab-bar",
      });

      ctx.keybindingManager.register({
        id: "tab-bar.toggle-sidebar",
        keys: "Cmd+B",
        label: "Toggle Sidebar",
        extensionId: "tab-bar",
        handler: () => window.dispatchEvent(new Event(SIDEBAR_TOGGLE_EVENT)),
      });

      ctx.keybindingManager.register({
        id: "tab-bar.new-tab",
        keys: "Cmd+T",
        label: "New Session",
        extensionId: "tab-bar",
        handler: () => {
          window.dispatchEvent(new Event("tab-bar:new-tab-trigger"));
        },
      });

      ctx.keybindingManager.register({
        id: "tab-bar.new-terminal",
        keys: "Cmd+Shift+T",
        label: "New Terminal",
        extensionId: "tab-bar",
        handler: async () => {
          await invoke("create_session", {
            request: { provider_id: "terminal", title: "Terminal" },
          });
        },
      });

      ctx.keybindingManager.register({
        id: "tab-bar.rename-tab",
        keys: "F2",
        label: "Rename Tab",
        extensionId: "tab-bar",
        handler: () => {
          window.dispatchEvent(new Event("tab-bar:rename-active"));
        },
      });

      ctx.keybindingManager.register({
        id: "tab-bar.set-title",
        keys: "Ctrl+R",
        label: "Set Title",
        extensionId: "tab-bar",
        handler: () => {
          window.dispatchEvent(new Event(SET_TITLE_EVENT));
        },
      });

      ctx.keybindingManager.register({
        id: "tab-bar.close-tab",
        keys: "Cmd+W",
        label: "Close Tab",
        extensionId: "tab-bar",
        handler: async () => {
          const active = await invoke<string | null>("get_active_session");
          if (active) {
            invoke("close_session", { sessionId: active });
          }
        },
      });

      ctx.keybindingManager.register({
        id: "tab-bar.next-tab",
        keys: "Ctrl+Tab",
        label: "Next Tab",
        extensionId: "tab-bar",
        handler: () => cycleTabs(1),
      });

      ctx.keybindingManager.register({
        id: "tab-bar.prev-tab",
        keys: "Ctrl+Shift+Tab",
        label: "Previous Tab",
        extensionId: "tab-bar",
        handler: () => cycleTabs(-1),
      });

      ctx.keybindingManager.register({
        id: "tab-bar.next-active-tab",
        keys: "Cmd+Alt+ArrowRight",
        label: "Next Active Tab",
        extensionId: "tab-bar",
        handler: () => cycleTabs(1, "active"),
      });

      ctx.keybindingManager.register({
        id: "tab-bar.prev-active-tab",
        keys: "Cmd+Alt+ArrowLeft",
        label: "Previous Active Tab",
        extensionId: "tab-bar",
        handler: () => cycleTabs(-1, "active"),
      });

      ctx.keybindingManager.register({
        id: "tab-bar.next-your-turn-tab",
        keys: "Cmd+Shift+ArrowRight",
        label: "Next Your Turn Tab",
        extensionId: "tab-bar",
        handler: () => cycleTabs(1, "your_turn"),
      });

      ctx.keybindingManager.register({
        id: "tab-bar.prev-your-turn-tab",
        keys: "Cmd+Shift+ArrowLeft",
        label: "Previous Your Turn Tab",
        extensionId: "tab-bar",
        handler: () => cycleTabs(-1, "your_turn"),
      });

      ctx.keybindingManager.register({
        id: "tab-bar.fork-session",
        keys: "Cmd+Alt+K",
        label: "Fork Current Session",
        extensionId: "tab-bar",
        handler: async () => {
          const active = await invoke<string | null>("get_active_session");
          if (active) {
            try {
              await invoke("fork_active_session", { sessionId: active });
            } catch (err) {
              console.error("[tab-bar] Fork failed:", err);
            }
          }
        },
      });

      ctx.keybindingManager.register({
        id: "tab-bar.new-terminal-in-cwd",
        keys: "Cmd+Alt+T",
        label: "New Terminal in Current Directory",
        extensionId: "tab-bar",
        handler: async () => {
          const activeId = await invoke<string | null>("get_active_session");
          let workingDir: string | null = null;

          if (activeId) {
            const sessions = await invoke<SessionInfo[]>("list_sessions");
            const activeSession = sessions.find((s) => s.id === activeId);
            workingDir = activeSession?.working_directory ?? null;
          }

          if (workingDir) {
            const dirName = workingDir.split("/").filter(Boolean).pop() || workingDir;
            await invoke("create_session", {
              request: {
                provider_id: "terminal",
                title: dirName,
                working_directory: workingDir,
              },
            });
          } else {
            await invoke("create_session", {
              request: { provider_id: "terminal", title: "Terminal" },
            });
          }
        },
      });

      ctx.keybindingManager.register({
        id: "tab-bar.new-claude-in-cwd",
        keys: "Cmd+Alt+C",
        label: "New Claude in Current Directory",
        extensionId: "tab-bar",
        handler: async () => {
          const activeId = await invoke<string | null>("get_active_session");
          let workingDir: string | null = null;

          if (activeId) {
            const sessions = await invoke<SessionInfo[]>("list_sessions");
            const activeSession = sessions.find((s) => s.id === activeId);
            workingDir = activeSession?.working_directory ?? null;
          }

          if (workingDir) {
            const dirName = workingDir.split("/").filter(Boolean).pop() || workingDir;
            await invoke("create_session", {
              request: {
                provider_id: "claude-code",
                title: dirName,
                working_directory: workingDir,
              },
            });
          } else {
            const dir = await open({
              title: "Select Working Directory",
              directory: true,
            });
            if (!dir) return;
            const dirName = dir.split("/").filter(Boolean).pop() || dir;
            await invoke("create_session", {
              request: {
                provider_id: "claude-code",
                title: dirName,
                working_directory: dir,
              },
            });
          }
        },
      });

      for (let i = 1; i <= 9; i++) {
        ctx.keybindingManager.register({
          id: `tab-bar.switch-${i}`,
          keys: `Cmd+${i}`,
          label: `Switch to Tab ${i}`,
          extensionId: "tab-bar",
          handler: async () => {
            const sessions = await invoke<SessionInfo[]>("list_sessions");
            if (sessions[i - 1]) {
              invoke("set_active_session", { sessionId: sessions[i - 1].id });
            }
          },
        });
      }

      ctx.keybindingManager.register({
        id: "tab-bar.focus-search",
        keys: "Cmd+Shift+F",
        label: "Search History",
        extensionId: "tab-bar",
        handler: async () => {
          const input = document.querySelector("[data-search-bar]") as HTMLInputElement;
          if (input) input.focus();
        },
      });
    },
  };
}
