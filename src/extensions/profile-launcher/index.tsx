import React, { useState, useEffect, useRef, useCallback } from "react";
import { FrontendExtension } from "../../types/extension";
import { Profile, ProfileLaunchRequest } from "../../types/profile";
import { SLOTS } from "../../types/slots";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { InputField } from "../profiles/InputField";

// Module-level state (same pattern as command-palette)
let showLauncher = false;
let launcherListeners: Array<() => void> = [];

function notifyLauncher() {
  launcherListeners.forEach((l) => l());
}

function fuzzyMatch(query: string, text: string): boolean {
  const lower = text.toLowerCase();
  let qi = 0;
  for (let i = 0; i < lower.length && qi < query.length; i++) {
    if (lower[i] === query[qi]) qi++;
  }
  return qi === query.length;
}

type LauncherPhase = "search" | "inputs";

function ProfileLauncher() {
  const [visible, setVisible] = useState(false);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [phase, setPhase] = useState<LauncherPhase>("search");
  const [activeProfile, setActiveProfile] = useState<Profile | null>(null);
  const [inputValues, setInputValues] = useState<Record<string, string>>({});
  const [workingDir, setWorkingDir] = useState("");
  const [launching, setLaunching] = useState(false);
  const inputRef = useRef<HTMLInputElement>(null);
  const firstInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    const update = () => {
      const wasVisible = visible;
      setVisible(showLauncher);
      if (showLauncher) {
        setQuery("");
        setSelected(0);
        setPhase("search");
        setActiveProfile(null);
        setInputValues({});
        setWorkingDir("");
        setLaunching(false);
        invoke<Profile[]>("list_profiles").then(setProfiles).catch(console.error);
        setTimeout(() => inputRef.current?.focus(), 0);
      } else if (wasVisible) {
        const terminal = document.querySelector(".xterm-helper-textarea") as HTMLTextAreaElement;
        if (terminal) terminal.focus();
      }
    };
    launcherListeners.push(update);
    return () => {
      launcherListeners = launcherListeners.filter((l) => l !== update);
    };
  }, [visible]);

  const close = useCallback(() => {
    showLauncher = false;
    notifyLauncher();
  }, []);

  const launchProfile = useCallback(async (
    profile: Profile,
    values: Record<string, string>,
    dir?: string,
  ) => {
    setLaunching(true);
    try {
      const request: ProfileLaunchRequest = {
        profile_id: profile.id,
        input_values: values,
        working_directory: dir,
      };
      await invoke("launch_profile", { request });
      close();
    } catch (err) {
      console.error("[ProfileLauncher] Launch failed:", err);
      setLaunching(false);
    }
  }, [close]);

  const selectProfile = useCallback((profile: Profile) => {
    const needsInputs = profile.inputs.length > 0;
    const needsDir =
      !profile.working_directory ||
      profile.working_directory.type === "prompt";

    if (!needsInputs && !needsDir) {
      launchProfile(profile, {}, profile.working_directory?.path);
      return;
    }

    // Switch to inputs phase
    setActiveProfile(profile);
    setPhase("inputs");
    const defaults: Record<string, string> = {};
    for (const input of profile.inputs) {
      if (input.default) defaults[input.key] = input.default;
    }
    setInputValues(defaults);
    setWorkingDir(
      profile.working_directory?.type === "fixed"
        ? profile.working_directory.path || ""
        : "",
    );
    setTimeout(() => firstInputRef.current?.focus(), 0);
  }, [launchProfile]);

  const handleLaunch = useCallback(() => {
    if (!activeProfile) return;
    for (const input of activeProfile.inputs) {
      if (input.required && !inputValues[input.key]?.trim()) return;
    }
    const needsDir =
      !activeProfile.working_directory ||
      activeProfile.working_directory.type === "prompt";
    if (needsDir && !workingDir.trim()) return;
    launchProfile(activeProfile, inputValues, workingDir || undefined);
  }, [activeProfile, inputValues, workingDir, launchProfile]);

  const handleBrowseDir = async () => {
    const dir = await open({ title: "Select Working Directory", directory: true });
    if (dir) setWorkingDir(dir);
  };

  const searchQuery = query.toLowerCase();
  const filtered = profiles
    .filter((p) =>
      searchQuery ? fuzzyMatch(searchQuery, p.name) || (p.description && fuzzyMatch(searchQuery, p.description)) : true,
    )
    .sort((a, b) => {
      // Default profile always first
      if (a.is_default && !b.is_default) return -1;
      if (!a.is_default && b.is_default) return 1;
      if (searchQuery) {
        // Prioritize name matches over description-only matches
        const aNameMatch = fuzzyMatch(searchQuery, a.name);
        const bNameMatch = fuzzyMatch(searchQuery, b.name);
        if (aNameMatch && !bNameMatch) return -1;
        if (!aNameMatch && bNameMatch) return 1;
      }
      return a.name.localeCompare(b.name);
    });

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      if (phase === "inputs") {
        setPhase("search");
        setActiveProfile(null);
        setTimeout(() => inputRef.current?.focus(), 0);
      } else {
        close();
      }
    } else if (phase === "search") {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelected((s) => Math.min(s + 1, filtered.length - 1));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelected((s) => Math.max(s - 1, 0));
      } else if (e.key === "Enter" && filtered[selected]) {
        selectProfile(filtered[selected]);
      }
    } else if (phase === "inputs" && e.key === "Enter") {
      handleLaunch();
    }
  };

  if (!visible) return null;

  return (
    <div
      className="command-palette-backdrop"
      onClick={close}
      role="presentation"
    >
      <div
        className="command-palette quick-launcher"
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-label="Quick Profile Launcher"
        aria-modal="true"
        onKeyDown={handleKeyDown}
      >
        {phase === "search" ? (
          <>
            <input
              ref={inputRef}
              className="command-palette-input"
              placeholder="Launch profile..."
              value={query}
              onChange={(e) => { setQuery(e.target.value); setSelected(0); }}
            />
            <div className="command-palette-list">
              {filtered.map((profile, i) => (
                <div
                  key={profile.id}
                  className={`command-palette-item ${i === selected ? "selected" : ""}`}
                  onClick={() => selectProfile(profile)}
                  role="option"
                  aria-selected={i === selected}
                >
                  <span className="command-palette-tab-row">
                    <span className="command-label">{profile.name}</span>
                    {profile.description && (
                      <span className="command-palette-detail">{profile.description}</span>
                    )}
                  </span>
                  {profile.inputs.length > 0 && (
                    <span className="quick-launcher-badge">
                      {profile.inputs.length} input{profile.inputs.length > 1 ? "s" : ""}
                    </span>
                  )}
                </div>
              ))}
              {filtered.length === 0 && (
                <div className="command-palette-empty">
                  {profiles.length === 0 ? "No profiles yet" : "No matching profiles"}
                </div>
              )}
            </div>
          </>
        ) : activeProfile ? (
          <div className="quick-launcher-inputs">
            <div className="quick-launcher-header">
              <button
                className="quick-launcher-back"
                onClick={() => { setPhase("search"); setActiveProfile(null); setTimeout(() => inputRef.current?.focus(), 0); }}
              >
                &larr;
              </button>
              <span className="quick-launcher-profile-name">{activeProfile.name}</span>
            </div>
            <div className="quick-launcher-fields">
              {activeProfile.inputs.map((input, i) => (
                <div key={input.key} className="quick-launcher-field">
                  <label className="profiles-field-label">
                    {input.label}
                    {input.required && <span className="profiles-required">*</span>}
                  </label>
                  <InputField
                    input={input}
                    value={inputValues[input.key] || ""}
                    onChange={(v) => setInputValues((prev) => ({ ...prev, [input.key]: v }))}
                    onEnter={handleLaunch}
                    autoFocusRef={i === 0 ? firstInputRef : undefined}
                  />
                </div>
              ))}
              {(!activeProfile.working_directory ||
                activeProfile.working_directory.type === "prompt") && (
                <div className="quick-launcher-field">
                  <label className="profiles-field-label">Working Directory</label>
                  <div className="profiles-dir-row">
                    <input
                      ref={activeProfile.inputs.length === 0 ? firstInputRef : undefined}
                      className="profiles-field-input profiles-dir-input"
                      type="text"
                      placeholder="~/my-project"
                      value={workingDir}
                      onChange={(e) => setWorkingDir(e.target.value)}
                    />
                    <button
                      className="profiles-dir-browse"
                      onClick={handleBrowseDir}
                      title="Browse"
                    >
                      ...
                    </button>
                  </div>
                </div>
              )}
            </div>
            <div className="quick-launcher-footer">
              <button
                className="profiles-launch-btn"
                onClick={handleLaunch}
                disabled={launching}
              >
                {launching ? "Launching..." : "Launch"}
              </button>
            </div>
          </div>
        ) : null}
      </div>
    </div>
  );
}

export function createProfileLauncherExtension(): FrontendExtension {
  return {
    manifest: {
      id: "profile-launcher",
      name: "Quick Profile Launcher",
      version: "0.1.0",
      description: "Cmd+Shift+K quick profile launcher",
      dependencies: ["profiles"],
    },
    activate(ctx) {
      ctx.componentRegistry.register(SLOTS.OVERLAY, {
        id: "profile-launcher-overlay",
        component: ProfileLauncher,
        priority: 90,
        extensionId: "profile-launcher",
      });

      ctx.keybindingManager.register({
        id: "profile-launcher.quick-open",
        keys: "Cmd+Shift+K",
        label: "Quick Profile Launcher",
        extensionId: "profile-launcher",
        handler: () => {
          showLauncher = !showLauncher;
          notifyLauncher();
        },
      });
    },
  };
}
