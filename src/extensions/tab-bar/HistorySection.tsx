import React, { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ClaudeSession, DirectoryPreference } from "./types";

interface HistorySectionProps {
  onResume: (session: ClaudeSession) => void;
  onFork: (session: ClaudeSession) => void;
}

interface ProjectGroup {
  projectPath: string;
  displayName: string;
  sessions: ClaudeSession[];
  pinned: boolean;
  expanded: boolean;
}

// Helper to get display title for a session
function getSessionTitle(session: ClaudeSession): string {
  return session.summary || session.first_prompt || `Session ${session.session_id.slice(0, 8)}`;
}

// Truncate title if it's too long (e.g., raw prompts)
function truncateTitle(title: string, maxLength: number = 60): string {
  if (title.length <= maxLength) return title;
  return title.slice(0, maxLength) + "...";
}

export function HistorySection({ onResume, onFork }: HistorySectionProps) {
  const [sessions, setSessions] = useState<ClaudeSession[]>([]);
  const [preferences, setPreferences] = useState<DirectoryPreference[]>([]);
  const [collapsed, setCollapsed] = useState(false);
  const [recentExpanded, setRecentExpanded] = useState(true);
  const [expandedProjects, setExpandedProjects] = useState<Set<string>>(new Set());
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    session?: ClaudeSession;
    projectPath?: string;
  } | null>(null);

  const loadSessions = useCallback(async () => {
    try {
      const list = await invoke<ClaudeSession[]>("list_claude_sessions", {
        filter: { limit: 200, include_hidden: false },
      });
      setSessions(list);
    } catch (err) {
      console.error("[HistorySection] Failed to load:", err);
    }
  }, []);

  const loadPreferences = useCallback(async () => {
    try {
      const prefs = await invoke<DirectoryPreference[]>("get_directory_preferences");
      setPreferences(prefs);
    } catch (err) {
      console.error("[HistorySection] Failed to load preferences:", err);
    }
  }, []);

  useEffect(() => {
    // Load sessions directly from Claude's files
    loadSessions();
    loadPreferences();

    let mounted = true;
    const unsubs: Array<() => void> = [];
    let delayedRefreshTimeout: ReturnType<typeof setTimeout> | null = null;

    // Refresh on session create/close (Claude Code updates sessions-index.json)
    listen("core-event", (e: { payload: { topic: string } }) => {
      if (!mounted) return;
      if (e.payload.topic === "session.closed" || e.payload.topic === "session.created") {
        loadSessions();
        // Refresh again after delay - Claude Code may update files asynchronously
        if (e.payload.topic === "session.created") {
          delayedRefreshTimeout = setTimeout(() => { if (mounted) loadSessions(); }, 2000);
        }
      }
    }).then((u) => {
      if (!mounted) { u(); return; }
      unsubs.push(u);
    });

    return () => {
      mounted = false;
      unsubs.forEach((u) => u());
      if (delayedRefreshTimeout) clearTimeout(delayedRefreshTimeout);
    };
  }, [loadSessions, loadPreferences]);

  const handleSessionContextMenu = (e: React.MouseEvent, session: ClaudeSession) => {
    e.preventDefault();
    e.stopPropagation();
    const menuHeight = 100;
    const menuWidth = 110;
    const x = Math.min(e.clientX, window.innerWidth - menuWidth - 8);
    const y = Math.min(e.clientY, window.innerHeight - menuHeight - 8);
    setContextMenu({ x, y, session });
  };

  const handleProjectContextMenu = (e: React.MouseEvent, projectPath: string) => {
    e.preventDefault();
    e.stopPropagation();
    const menuHeight = 80;
    const menuWidth = 120;
    const x = Math.min(e.clientX, window.innerWidth - menuWidth - 8);
    const y = Math.min(e.clientY, window.innerHeight - menuHeight - 8);
    setContextMenu({ x, y, projectPath });
  };

  const handleResume = () => {
    if (contextMenu?.session) {
      onResume(contextMenu.session);
      setContextMenu(null);
    }
  };

  const handleFork = () => {
    if (contextMenu?.session) {
      onFork(contextMenu.session);
      setContextMenu(null);
    }
  };

  const togglePin = async () => {
    if (contextMenu?.projectPath) {
      const pref = preferences.find(p => p.project_path === contextMenu.projectPath);
      const newPinned = !(pref?.pinned ?? false);
      try {
        await invoke("set_directory_preference", {
          projectPath: contextMenu.projectPath,
          pinned: newPinned,
        });
        await loadPreferences();
      } catch (err) {
        console.error("[HistorySection] Failed to toggle pin:", err);
      }
      setContextMenu(null);
    }
  };

  const hideProject = async () => {
    if (contextMenu?.projectPath) {
      try {
        await invoke("set_directory_preference", {
          projectPath: contextMenu.projectPath,
          hidden: true,
        });
        await loadPreferences();
        await loadSessions();
      } catch (err) {
        console.error("[HistorySection] Failed to hide project:", err);
      }
      setContextMenu(null);
    }
  };

  const deleteSession = async () => {
    if (contextMenu?.session) {
      try {
        await invoke("delete_history_session", {
          sessionId: contextMenu.session.session_id,
        });
        await loadSessions();
      } catch (err) {
        console.error("[HistorySection] Failed to delete session:", err);
      }
      setContextMenu(null);
    }
  };

  const deleteProjectSessions = async () => {
    if (contextMenu?.projectPath) {
      const count = sessions.filter(s => s.project_path === contextMenu.projectPath).length;
      if (!confirm(`Delete all ${count} sessions from this project?`)) {
        setContextMenu(null);
        return;
      }
      try {
        await invoke("delete_project_sessions", {
          projectPath: contextMenu.projectPath,
        });
        await loadSessions();
      } catch (err) {
        console.error("[HistorySection] Failed to delete project sessions:", err);
      }
      setContextMenu(null);
    }
  };

  const toggleProjectExpand = (projectPath: string) => {
    setExpandedProjects(prev => {
      const next = new Set(prev);
      if (next.has(projectPath)) {
        next.delete(projectPath);
      } else {
        next.add(projectPath);
      }
      return next;
    });
  };

  // Close context menu on outside click
  useEffect(() => {
    const handler = () => setContextMenu(null);
    if (contextMenu) {
      document.addEventListener("click", handler);
      return () => document.removeEventListener("click", handler);
    }
  }, [contextMenu]);

  // Group sessions by project
  const projectGroups = groupByProject(sessions, preferences, expandedProjects);

  // Get recent sessions (top 5 most recent across all projects)
  const recentSessions = [...sessions]
    .sort((a, b) => b.modified_at.localeCompare(a.modified_at))
    .slice(0, 5);

  if (sessions.length === 0) {
    return null;
  }

  return (
    <div className="history-section">
      <div className="history-section-header" onClick={() => setCollapsed(!collapsed)}>
        <span className={`history-section-chevron ${collapsed ? "collapsed" : ""}`}>
          &#9662;
        </span>
        <span>History</span>
        <button
          className="history-refresh-btn"
          onClick={(e) => { e.stopPropagation(); loadSessions(); }}
          title="Refresh"
        >
          &#8635;
        </button>
      </div>

      {/* Recent Sessions Section */}
      {!collapsed && recentSessions.length > 0 && (
        <div className="history-project-group recent-group">
          <div
            className="history-project-header"
            onClick={() => setRecentExpanded(!recentExpanded)}
          >
            <span className={`history-section-chevron ${!recentExpanded ? "collapsed" : ""}`}>
              &#9662;
            </span>
            <span className="history-project-name">Recent</span>
            <span className="history-project-count">{recentSessions.length}</span>
          </div>
          {recentExpanded && recentSessions.map((session) => (
            <div
              key={`recent-${session.session_id}`}
              className="history-item"
              onContextMenu={(e) => handleSessionContextMenu(e, session)}
              onClick={() => onResume(session)}
              title={getSessionTitle(session)}
            >
              <div className="history-item-info">
                <span className="history-item-title">
                  {truncateTitle(getSessionTitle(session))}
                </span>
                <span className="history-item-project" title={session.project_path}>
                  {getProjectDisplayName(session.project_path)}
                </span>
              </div>
              <span className="history-item-time">
                {formatShortTime(session.modified_at)}
              </span>
              <div className="history-item-actions">
                <button
                  className="history-item-action"
                  onClick={(e) => { e.stopPropagation(); onResume(session); }}
                  title="Resume"
                >
                  &#9654;
                </button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Project Groups */}
      {!collapsed && projectGroups.map((group) => (
        <div key={group.projectPath} className={`history-project-group ${group.pinned ? "pinned" : ""}`}>
          <div
            className="history-project-header"
            onClick={() => toggleProjectExpand(group.projectPath)}
            onContextMenu={(e) => handleProjectContextMenu(e, group.projectPath)}
          >
            <span className={`history-section-chevron ${!group.expanded ? "collapsed" : ""}`}>
              &#9662;
            </span>
            {group.pinned && <span className="pin-indicator" title="Pinned">&#9733;</span>}
            <span className="history-project-name" title={group.projectPath}>
              {group.displayName}
            </span>
            <span className="history-project-count">{group.sessions.length}</span>
          </div>
          {group.expanded && group.sessions.map((session) => (
            <div
              key={session.session_id}
              className="history-item"
              onContextMenu={(e) => handleSessionContextMenu(e, session)}
              onClick={() => onResume(session)}
              title={getSessionTitle(session)}
            >
              <div className="history-item-info">
                <span className="history-item-title">
                  {truncateTitle(getSessionTitle(session))}
                </span>
                {session.git_branch && (
                  <span className="history-item-branch" title={`Branch: ${session.git_branch}`}>
                    {session.git_branch}
                  </span>
                )}
              </div>
              <span className="history-item-time">
                {formatShortTime(session.modified_at)}
              </span>
              <div className="history-item-actions">
                <button
                  className="history-item-action"
                  onClick={(e) => { e.stopPropagation(); onResume(session); }}
                  title="Resume"
                >
                  &#9654;
                </button>
              </div>
            </div>
          ))}
        </div>
      ))}

      {contextMenu && (
        <div
          className="history-context-menu"
          style={{ top: contextMenu.y, left: contextMenu.x }}
        >
          {contextMenu.session ? (
            <>
              <div className="history-context-item" onClick={handleResume}>
                Resume
              </div>
              <div className="history-context-item" onClick={handleFork}>
                Fork
              </div>
              <div className="history-context-item history-context-delete" onClick={deleteSession}>
                Delete
              </div>
            </>
          ) : contextMenu.projectPath ? (
            <>
              <div className="history-context-item" onClick={togglePin}>
                {preferences.find(p => p.project_path === contextMenu.projectPath)?.pinned
                  ? "Unpin"
                  : "Pin to top"}
              </div>
              <div className="history-context-item" onClick={hideProject}>
                Hide project
              </div>
              <div className="history-context-item history-context-delete" onClick={deleteProjectSessions}>
                Delete all sessions
              </div>
            </>
          ) : null}
        </div>
      )}
    </div>
  );
}

function groupByProject(
  sessions: ClaudeSession[],
  preferences: DirectoryPreference[],
  expandedProjects: Set<string>
): ProjectGroup[] {
  const groupMap = new Map<string, ClaudeSession[]>();

  for (const session of sessions) {
    const existing = groupMap.get(session.project_path) || [];
    existing.push(session);
    groupMap.set(session.project_path, existing);
  }

  const groups: ProjectGroup[] = [];

  for (const [projectPath, projectSessions] of groupMap) {
    const pref = preferences.find(p => p.project_path === projectPath);
    const displayName = pref?.display_name || getProjectDisplayName(projectPath);
    groups.push({
      projectPath,
      displayName,
      sessions: projectSessions,
      pinned: pref?.pinned ?? false,
      expanded: expandedProjects.has(projectPath),
    });
  }

  // Sort: pinned first, then by most recent session
  groups.sort((a, b) => {
    if (a.pinned && !b.pinned) return -1;
    if (!a.pinned && b.pinned) return 1;
    const aLatest = a.sessions[0]?.modified_at ?? "";
    const bLatest = b.sessions[0]?.modified_at ?? "";
    return bLatest.localeCompare(aLatest);
  });

  return groups;
}

function getProjectDisplayName(projectPath: string): string {
  const parts = projectPath.split("/").filter(Boolean);
  return parts[parts.length - 1] || projectPath;
}

function formatShortTime(isoString: string): string {
  const date = new Date(isoString);
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());

  if (date >= today) {
    return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }
  return date.toLocaleDateString([], { month: "short", day: "numeric" });
}
