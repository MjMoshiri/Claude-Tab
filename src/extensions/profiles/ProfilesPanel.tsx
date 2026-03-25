import React, { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Profile, ProfileLaunchRequest, Pack } from "../../types/profile";
import { ProfileEditor } from "./ProfileEditor";
import { InputField } from "./InputField";

let showProfiles = false;
let profilesListeners: Array<() => void> = [];

function notifyProfiles() {
  profilesListeners.forEach((l) => l());
}

export function setProfilesPanelVisible(visible: boolean) {
  showProfiles = visible;
  notifyProfiles();
}

export function getProfilesPanelVisible() {
  return showProfiles;
}

export function ProfilesPanel() {
  const [visible, setVisible] = useState(false);
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [inputValues, setInputValues] = useState<Record<string, string>>({});
  const [workingDir, setWorkingDir] = useState("");
  const [showEditor, setShowEditor] = useState(false);
  const [editingProfile, setEditingProfile] = useState<Profile | null>(null);
  const [launching, setLaunching] = useState(false);
  const launchingRef = useRef(false);
  const [packs, setPacks] = useState<Pack[]>([]);
  const [showPacks, setShowPacks] = useState(false);
  const [editingPack, setEditingPack] = useState<Pack | null>(null);
  const [showPackEditor, setShowPackEditor] = useState(false);
  const [batchMode, setBatchMode] = useState(false);
  const [batchDelimiter, setBatchDelimiter] = useState("-");
  const [batchRawText, setBatchRawText] = useState<Record<string, string>>({});

  useEffect(() => {
    const update = () => {
      const wasVisible = visible;
      setVisible(showProfiles);
      if (showProfiles) {
        loadProfiles();
        loadPacks();
        setExpandedId(null);
        setShowEditor(false);
        setEditingProfile(null);
        setShowPackEditor(false);
        setBatchMode(false);
      } else if (wasVisible) {
        // Return focus to terminal when profiles closes
        const terminal = document.querySelector(".xterm-helper-textarea") as HTMLTextAreaElement;
        if (terminal) terminal.focus();
      }
    };
    profilesListeners.push(update);
    return () => {
      profilesListeners = profilesListeners.filter((l) => l !== update);
    };
  }, [visible]);

  const loadProfiles = useCallback(async () => {
    try {
      const list = await invoke<Profile[]>("list_profiles");
      setProfiles(list);
    } catch (err) {
      console.error("[Profiles] Failed to load:", err);
    }
  }, []);

  const loadPacks = useCallback(async () => {
    try {
      const list = await invoke<Pack[]>("list_packs");
      setPacks(list);
    } catch (err) {
      console.error("[Profiles] Failed to load packs:", err);
    }
  }, []);

  const close = () => {
    showProfiles = false;
    notifyProfiles();
  };

  const handleCardClick = (profile: Profile) => {
    // If no inputs needed and fixed directory, launch immediately
    const needsInputs = profile.inputs.length > 0;
    const needsDir =
      !profile.working_directory ||
      profile.working_directory.type === "prompt";

    if (!needsInputs && !needsDir) {
      launchProfile(profile, {}, profile.working_directory?.path);
      return;
    }

    if (expandedId === profile.id) {
      setExpandedId(null);
    } else {
      setExpandedId(profile.id);
      // Initialize input values with defaults
      const defaults: Record<string, string> = {};
      for (const input of profile.inputs) {
        if (input.default) {
          defaults[input.key] = input.default;
        }
      }
      setInputValues(defaults);
      setWorkingDir(
        profile.working_directory?.type === "fixed"
          ? profile.working_directory.path || ""
          : ""
      );
    }
  };

  const launchProfile = async (
    profile: Profile,
    values: Record<string, string>,
    dir?: string
  ) => {
    if (launchingRef.current) return;
    launchingRef.current = true;
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
      console.error("[Profiles] Launch failed:", err);
    } finally {
      setLaunching(false);
      launchingRef.current = false;
    }
  };

  const handleLaunch = (profile: Profile) => {
    // Validate required inputs
    for (const input of profile.inputs) {
      if (input.required && !inputValues[input.key]?.trim()) {
        return;
      }
    }
    const needsDir =
      !profile.working_directory ||
      profile.working_directory.type === "prompt";
    if (needsDir && !workingDir.trim()) {
      return;
    }
    launchProfile(profile, inputValues, workingDir || undefined);
  };

  const handleBrowseDir = async () => {
    const dir = await open({
      title: "Select Working Directory",
      directory: true,
    });
    if (dir) {
      setWorkingDir(dir);
    }
  };

  const handleInputChange = (key: string, value: string) => {
    setInputValues((prev) => ({ ...prev, [key]: value }));
  };

  const handleNewProfile = () => {
    setEditingProfile(null);
    setShowEditor(true);
  };

  const handleEditProfile = (e: React.MouseEvent, profile: Profile) => {
    e.stopPropagation();
    setEditingProfile(profile);
    setShowEditor(true);
  };

  const handleDeleteProfile = async (e: React.MouseEvent, profileId: string) => {
    e.stopPropagation();
    try {
      await invoke("delete_profile", { profileId });
      loadProfiles();
    } catch (err) {
      console.error("[Profiles] Delete failed:", err);
    }
  };

  const handleEditorSave = async (profile: Profile) => {
    try {
      await invoke("save_profile", { profile });
      setShowEditor(false);
      loadProfiles();
    } catch (err) {
      console.error("[Profiles] Save failed:", err);
    }
  };

  const handleEditorClose = () => {
    setShowEditor(false);
  };

  const handleCopyProfile = async (e: React.MouseEvent, profile: Profile) => {
    e.stopPropagation();
    const copy: Profile = {
      ...profile,
      id: profile.id + "-copy-" + Date.now(),
      name: profile.name + " (Copy)",
      is_default: false,
    };
    try {
      await invoke("save_profile", { profile: copy });
      loadProfiles();
    } catch (err) {
      console.error("[Profiles] Copy failed:", err);
    }
  };

  // --- Pack handlers ---
  const handleNewPack = () => {
    setEditingPack(null);
    setShowPackEditor(true);
  };

  const handleEditPack = (pack: Pack) => {
    setEditingPack(pack);
    setShowPackEditor(true);
  };

  const handleDeletePack = async (e: React.MouseEvent, packId: string) => {
    e.stopPropagation();
    try {
      await invoke("delete_pack", { packId });
      loadPacks();
    } catch (err) {
      console.error("[Profiles] Delete pack failed:", err);
    }
  };

  const handlePackSave = async (pack: Pack) => {
    try {
      await invoke("save_pack", { pack });
      setShowPackEditor(false);
      loadPacks();
    } catch (err) {
      console.error("[Profiles] Save pack failed:", err);
    }
  };

  const handleLaunchPack = async (pack: Pack) => {
    setLaunching(true);
    try {
      for (const profileId of pack.profile_ids) {
        const profile = profiles.find((p) => p.id === profileId);
        if (!profile) continue;
        // Launch each profile with empty inputs (profiles in packs should have defaults or no required inputs)
        const request: ProfileLaunchRequest = {
          profile_id: profileId,
          input_values: {},
        };
        await invoke("launch_profile", { request });
      }
      close();
    } catch (err) {
      console.error("[Profiles] Pack launch failed:", err);
    } finally {
      setLaunching(false);
    }
  };

  // --- Batch launch ---
  const parseBatchValues = (raw: string): string[] => {
    if (!batchDelimiter.trim()) return [raw.trim()].filter(Boolean);
    return raw.split(batchDelimiter).map((s) => s.trim()).filter(Boolean);
  };

  const getBatchCount = (profile: Profile): number => {
    const batchKeys: string[] = [];
    const batchValues: Record<string, string[]> = {};
    for (const input of profile.inputs) {
      const raw = batchRawText[input.key] || "";
      const values = parseBatchValues(raw);
      if (values.length > 1) {
        batchKeys.push(input.key);
        batchValues[input.key] = values;
      }
    }
    if (batchKeys.length === 0) return 1;
    return batchKeys.reduce((acc, key) => acc * batchValues[key].length, 1);
  };

  const handleBatchLaunch = async (profile: Profile) => {
    const batchKeys: string[] = [];
    const staticInputs: Record<string, string> = {};
    const batchValues: Record<string, string[]> = {};

    for (const input of profile.inputs) {
      const raw = batchRawText[input.key] || "";
      const values = parseBatchValues(raw);
      if (values.length > 1) {
        batchKeys.push(input.key);
        batchValues[input.key] = values;
      } else {
        staticInputs[input.key] = values[0] || inputValues[input.key] || "";
      }
    }

    if (batchKeys.length === 0) {
      handleLaunch(profile);
      return;
    }

    // Generate cartesian product
    let combinations: Record<string, string>[] = [{}];
    for (const key of batchKeys) {
      const newCombinations: Record<string, string>[] = [];
      for (const combo of combinations) {
        for (const value of batchValues[key]) {
          newCombinations.push({ ...combo, [key]: value });
        }
      }
      combinations = newCombinations;
    }

    setLaunching(true);
    try {
      for (const combo of combinations) {
        const mergedValues = { ...staticInputs, ...combo };
        const request: ProfileLaunchRequest = {
          profile_id: profile.id,
          input_values: mergedValues,
          working_directory: workingDir || undefined,
        };
        await invoke("launch_profile", { request });
      }
      close();
    } catch (err) {
      console.error("[Profiles] Batch launch failed:", err);
    } finally {
      setLaunching(false);
    }
  };

  if (!visible) return null;

  if (showEditor) {
    return (
      <div className="profiles-backdrop" onClick={close} role="presentation">
        <div className="profiles-panel profiles-panel-wide" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Edit profile" aria-modal="true">
          <ProfileEditor
            profile={editingProfile}
            onSave={handleEditorSave}
            onClose={handleEditorClose}
          />
        </div>
      </div>
    );
  }

  if (showPackEditor) {
    return (
      <div className="profiles-backdrop" onClick={close} role="presentation">
        <div className="profiles-panel profiles-panel-wide" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Edit pack" aria-modal="true">
          <PackEditor
            pack={editingPack}
            profiles={profiles}
            onSave={handlePackSave}
            onClose={() => setShowPackEditor(false)}
          />
        </div>
      </div>
    );
  }

  return (
    <div className="profiles-backdrop" onClick={close} role="presentation">
      <div className="profiles-panel" onClick={(e) => e.stopPropagation()} role="dialog" aria-label="Profiles" aria-modal="true">
        <div className="profiles-header">
          <span className="profiles-title">Profiles</span>
          <div className="profiles-header-tabs">
            <button
              className={`profiles-tab ${!showPacks ? "profiles-tab-active" : ""}`}
              onClick={() => setShowPacks(false)}
            >
              Profiles
            </button>
            <button
              className={`profiles-tab ${showPacks ? "profiles-tab-active" : ""}`}
              onClick={() => setShowPacks(true)}
            >
              Packs
            </button>
          </div>
          <button className="profiles-close" onClick={close} aria-label="Close profiles">
            &times;
          </button>
        </div>

        {!showPacks ? (
          <>
            <div className="profiles-list">
              {profiles.map((profile) => (
                <div key={profile.id} className="profiles-card-wrapper">
                  <div
                    className={`profiles-card ${expandedId === profile.id ? "profiles-card-expanded" : ""}`}
                    onClick={() => handleCardClick(profile)}
                  >
                    <div className="profiles-card-header">
                      <div className="profiles-card-info">
                        <span className="profiles-card-name">
                          {profile.name}
                          {profile.dangerously_skip_permissions && (
                            <span className="profiles-danger-badge" title="Bypass permissions">!</span>
                          )}
                        </span>
                        {profile.description && (
                          <span className="profiles-card-desc">{profile.description}</span>
                        )}
                      </div>
                      <div className="profiles-card-actions">
                        <button
                          className="profiles-card-action"
                          onClick={(e) => handleCopyProfile(e, profile)}
                          title="Copy"
                        >
                          &#9112;
                        </button>
                        <button
                          className="profiles-card-action"
                          onClick={(e) => handleEditProfile(e, profile)}
                          title="Edit"
                        >
                          &#9998;
                        </button>
                        <button
                          className="profiles-card-action profiles-card-action-danger"
                          onClick={(e) => handleDeleteProfile(e, profile.id)}
                          title="Delete"
                        >
                          &times;
                        </button>
                      </div>
                    </div>
                  </div>
                  {expandedId === profile.id && (
                    <div className="profiles-launch-form">
                      {profile.inputs.length > 0 && (
                        <div className="profiles-batch-toggle">
                          <label className="profiles-radio">
                            <input
                              type="checkbox"
                              checked={batchMode}
                              onChange={(e) => {
                                setBatchMode(e.target.checked);
                                if (e.target.checked) {
                                  const initial: Record<string, string> = {};
                                  for (const input of profile.inputs) {
                                    initial[input.key] = inputValues[input.key] || "";
                                  }
                                  setBatchRawText(initial);
                                }
                              }}
                            />
                            Batch mode
                            <span className="profiles-field-hint">Launch multiple sessions</span>
                          </label>
                          {batchMode && (
                            <div className="profiles-batch-delimiter">
                              <label className="profiles-field-hint">Delimiter:</label>
                              <input
                                className="profiles-field-input profiles-delimiter-input"
                                type="text"
                                value={batchDelimiter}
                                onChange={(e) => setBatchDelimiter(e.target.value)}
                              />
                            </div>
                          )}
                        </div>
                      )}
                      {profile.inputs.map((input) => (
                        <div key={input.key} className="profiles-field">
                          <label className="profiles-field-label">
                            {input.label}
                            {input.required && <span className="profiles-required">*</span>}
                          </label>
                          {batchMode ? (
                            <div className="profiles-batch-values">
                              <textarea
                                className="profiles-field-input profiles-batch-textarea"
                                placeholder={`Paste values separated by "${batchDelimiter}"`}
                                value={batchRawText[input.key] || ""}
                                onChange={(e) =>
                                  setBatchRawText((prev) => ({ ...prev, [input.key]: e.target.value }))
                                }
                                rows={3}
                              />
                              <span className="profiles-batch-count">
                                {parseBatchValues(batchRawText[input.key] || "").length} value(s)
                              </span>
                            </div>
                          ) : (
                            <InputField
                              input={input}
                              value={inputValues[input.key] || ""}
                              onChange={(v) => handleInputChange(input.key, v)}
                              onEnter={() => handleLaunch(profile)}
                            />
                          )}
                        </div>
                      ))}
                      {(!profile.working_directory ||
                        profile.working_directory.type === "prompt") && (
                        <div className="profiles-field">
                          <label className="profiles-field-label">Working Directory</label>
                          <div className="profiles-dir-row">
                            <input
                              className="profiles-field-input profiles-dir-input"
                              type="text"
                              placeholder="~/my-project"
                              value={workingDir}
                              onChange={(e) => setWorkingDir(e.target.value)}
                              onKeyDown={(e) => {
                                if (e.key === "Enter") handleLaunch(profile);
                              }}
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
                      {batchMode ? (
                        <button
                          className="profiles-launch-btn profiles-launch-batch"
                          onClick={() => handleBatchLaunch(profile)}
                          disabled={launching}
                        >
                          {launching ? "Launching..." : `Batch Launch (${getBatchCount(profile)} sessions)`}
                        </button>
                      ) : (
                        <button
                          className="profiles-launch-btn"
                          onClick={() => handleLaunch(profile)}
                          disabled={launching}
                        >
                          {launching ? "Launching..." : "Launch"}
                        </button>
                      )}
                    </div>
                  )}
                </div>
              ))}
              {profiles.length === 0 && (
                <div className="profiles-empty">
                  No profiles yet. Create one to get started.
                </div>
              )}
            </div>
            <div className="profiles-footer">
              <button className="profiles-new-btn" onClick={handleNewProfile}>
                + New Profile
              </button>
            </div>
          </>
        ) : (
          <>
            <div className="profiles-list">
              {packs.map((pack) => (
                <div key={pack.id} className="profiles-card-wrapper">
                  <div className="profiles-card">
                    <div className="profiles-card-header">
                      <div className="profiles-card-info">
                        <span className="profiles-card-name">{pack.name}</span>
                        {pack.description && (
                          <span className="profiles-card-desc">{pack.description}</span>
                        )}
                        <span className="profiles-card-desc">
                          {pack.profile_ids.length} profile{pack.profile_ids.length !== 1 ? "s" : ""}:
                          {" "}{pack.profile_ids.map((id) => profiles.find((p) => p.id === id)?.name || id).join(", ")}
                        </span>
                      </div>
                      <div className="profiles-card-actions">
                        <button
                          className="profiles-card-action profiles-card-action-launch"
                          onClick={() => handleLaunchPack(pack)}
                          title="Launch All"
                          disabled={launching}
                        >
                          &#9654;
                        </button>
                        <button
                          className="profiles-card-action"
                          onClick={() => handleEditPack(pack)}
                          title="Edit"
                        >
                          &#9998;
                        </button>
                        <button
                          className="profiles-card-action profiles-card-action-danger"
                          onClick={(e) => handleDeletePack(e, pack.id)}
                          title="Delete"
                        >
                          &times;
                        </button>
                      </div>
                    </div>
                  </div>
                </div>
              ))}
              {packs.length === 0 && (
                <div className="profiles-empty">
                  No packs yet. Create one to group profiles together.
                </div>
              )}
            </div>
            <div className="profiles-footer">
              <button className="profiles-new-btn" onClick={handleNewPack}>
                + New Pack
              </button>
            </div>
          </>
        )}
      </div>
    </div>
  );
}

// --- Pack Editor ---
function PackEditor({
  pack,
  profiles,
  onSave,
  onClose,
}: {
  pack: Pack | null;
  profiles: Profile[];
  onSave: (pack: Pack) => void;
  onClose: () => void;
}) {
  const [name, setName] = useState(pack?.name || "");
  const [description, setDescription] = useState(pack?.description || "");
  const [selectedIds, setSelectedIds] = useState<Set<string>>(
    new Set(pack?.profile_ids || [])
  );

  const generateId = (n: string) =>
    n.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");

  const toggleProfile = (id: string) => {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const handleSave = () => {
    if (!name.trim() || selectedIds.size === 0) return;
    onSave({
      id: pack?.id || generateId(name),
      name: name.trim(),
      description: description.trim() || undefined,
      profile_ids: Array.from(selectedIds),
    });
  };

  return (
    <div className="profiles-editor">
      <div className="profiles-header">
        <span className="profiles-title">
          {pack ? "Edit Pack" : "New Pack"}
        </span>
        <button className="profiles-close" onClick={onClose} aria-label="Close editor">
          &times;
        </button>
      </div>
      <div className="profiles-editor-body">
        <div className="profiles-field">
          <label className="profiles-field-label">Name<span className="profiles-required">*</span></label>
          <input
            className="profiles-field-input"
            type="text"
            placeholder="e.g. PR Review Pipeline"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </div>
        <div className="profiles-field">
          <label className="profiles-field-label">Description</label>
          <input
            className="profiles-field-input"
            type="text"
            placeholder="What does this pack do?"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
          />
        </div>
        <div className="profiles-section">
          <div className="profiles-section-header">
            <span>Profiles in Pack</span>
          </div>
          <div className="profiles-pack-list">
            {profiles.map((profile) => (
              <label key={profile.id} className="profiles-pack-item">
                <input
                  type="checkbox"
                  checked={selectedIds.has(profile.id)}
                  onChange={() => toggleProfile(profile.id)}
                />
                <span>{profile.name}</span>
              </label>
            ))}
          </div>
        </div>
      </div>
      <div className="profiles-editor-footer">
        <button className="profiles-cancel-btn" onClick={onClose}>Cancel</button>
        <button
          className="profiles-save-btn"
          onClick={handleSave}
          disabled={!name.trim() || selectedIds.size === 0}
        >
          Save
        </button>
      </div>
    </div>
  );
}
