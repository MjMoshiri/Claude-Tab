import { useState, useEffect } from "react";
import { Profile, ProfileInput, WorkingDirConfig } from "../../types/profile";
import { SkillPicker } from "./SkillPicker";
import { SystemPromptPicker } from "./SystemPromptPicker";

interface ProfileEditorProps {
  profile: Profile | null;
  onSave: (profile: Profile) => void;
  onClose: () => void;
}

function generateId(name: string): string {
  return name
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
}

/** Extract {{key}} placeholders from a prompt template */
function extractTemplateKeys(template: string): string[] {
  const regex = /\{\{(\w+)\}\}/g;
  const keys: string[] = [];
  const seen = new Set<string>();
  let match;
  while ((match = regex.exec(template)) !== null) {
    const key = match[1];
    if (!seen.has(key)) {
      seen.add(key);
      keys.push(key);
    }
  }
  return keys;
}

/** Convert a key like "pr_url" to a label like "Pr Url" */
function keyToLabel(key: string): string {
  return key
    .replace(/[_-]+/g, " ")
    .replace(/\b\w/g, (c) => c.toUpperCase());
}

export function ProfileEditor({ profile, onSave, onClose }: ProfileEditorProps) {
  const [name, setName] = useState(profile?.name || "");
  const [description, setDescription] = useState(profile?.description || "");
  const [promptTemplate, setPromptTemplate] = useState(profile?.prompt_template || "");
  const [inputs, setInputs] = useState<ProfileInput[]>(profile?.inputs || []);
  const [workingDirType, setWorkingDirType] = useState<"prompt" | "fixed" | "from_input">(
    profile?.working_directory?.type || "prompt"
  );
  const [fixedPath, setFixedPath] = useState(
    profile?.working_directory?.type === "fixed" ? profile.working_directory.path || "" : ""
  );
  const [fromInputKey, setFromInputKey] = useState(
    profile?.working_directory?.type === "from_input" ? profile.working_directory.key || "" : ""
  );
  const MODEL_OPTIONS = ["", "sonnet", "opus", "haiku"] as const;
  const isCustomModel = profile?.model && !MODEL_OPTIONS.includes(profile.model as typeof MODEL_OPTIONS[number]);
  const [model, setModel] = useState(profile?.model || "");
  const [useCustomModel, setUseCustomModel] = useState(isCustomModel);
  const [allowedTools, setAllowedTools] = useState(
    profile?.allowed_tools?.join(", ") || ""
  );
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [selectedSkills, setSelectedSkills] = useState<Set<string>>(
    new Set(profile?.skills || [])
  );
  const [systemPromptFile, setSystemPromptFile] = useState<string | null>(
    profile?.system_prompt_file || null
  );
  const [isDefault, setIsDefault] = useState(profile?.is_default || false);

  // Auto-detect inputs from {{key}} in prompt template
  useEffect(() => {
    const templateKeys = extractTemplateKeys(promptTemplate);
    if (templateKeys.length === 0) return;

    setInputs((prev) => {
      const existingByKey = new Map(prev.map((inp) => [inp.key, inp]));
      const merged: ProfileInput[] = [];

      // Add template-detected inputs (preserve existing settings if key matches)
      for (const key of templateKeys) {
        if (existingByKey.has(key)) {
          merged.push(existingByKey.get(key)!);
        } else {
          merged.push({
            key,
            label: keyToLabel(key),
            input_type: "text",
            required: true,
          });
        }
      }

      // Keep any manually-added inputs that aren't from the template
      const templateKeySet = new Set(templateKeys);
      for (const inp of prev) {
        if (!templateKeySet.has(inp.key) && inp.key && inp.label) {
          merged.push(inp);
        }
      }

      // Only update if actually changed
      const prevKeys = prev.map((i) => i.key).join(",");
      const newKeys = merged.map((i) => i.key).join(",");
      if (prevKeys === newKeys) return prev;
      return merged;
    });
  }, [promptTemplate]);

  const handleAddInput = () => {
    setInputs([
      ...inputs,
      { key: "", label: "", input_type: "text", required: true },
    ]);
  };

  const handleRemoveInput = (index: number) => {
    setInputs(inputs.filter((_, i) => i !== index));
  };

  const handleInputChange = (index: number, field: keyof ProfileInput, value: string | boolean) => {
    const updated = [...inputs];
    updated[index] = { ...updated[index], [field]: value };
    if (field === "label" && typeof value === "string") {
      updated[index].key = generateId(value);
    }
    setInputs(updated);
  };

  const handleSave = () => {
    if (!name.trim()) return;

    const id = profile?.id || generateId(name);

    let working_directory: WorkingDirConfig | undefined;
    if (workingDirType === "fixed" && fixedPath) {
      working_directory = { type: "fixed", path: fixedPath };
    } else if (workingDirType === "from_input" && fromInputKey) {
      working_directory = { type: "from_input", key: fromInputKey };
    } else {
      working_directory = { type: "prompt" };
    }

    const tools = allowedTools
      .split(",")
      .map((t) => t.trim())
      .filter(Boolean);

    const newProfile: Profile = {
      id,
      name: name.trim(),
      description: description.trim() || undefined,
      version: profile?.version || 1,
      working_directory,
      prompt_template: promptTemplate.trim() || undefined,
      auto_execute: false,
      system_prompt_file: systemPromptFile || undefined,
      skills: selectedSkills.size > 0 ? Array.from(selectedSkills) : undefined,
      allowed_tools: tools.length > 0 ? tools : undefined,
      model: model.trim() || undefined,
      system_prompt: profile?.system_prompt,
      inputs: inputs.filter((i) => i.key && i.label),
      tags: profile?.tags || [],
      is_default: isDefault,
    };

    onSave(newProfile);
  };

  return (
    <div className="profiles-editor">
      <div className="profiles-header">
        <span className="profiles-title">
          {profile ? "Edit Profile" : "New Profile"}
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
            placeholder="e.g. PR Review"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </div>

        <div className="profiles-field">
          <label className="profiles-field-label">Description</label>
          <input
            className="profiles-field-input"
            type="text"
            placeholder="What does this profile do?"
            value={description}
            onChange={(e) => setDescription(e.target.value)}
          />
        </div>

        <div className="profiles-field">
          <label className="profiles-radio">
            <input
              type="checkbox"
              checked={isDefault}
              onChange={(e) => setIsDefault(e.target.checked)}
            />
            Default profile
            <span className="profiles-field-hint">First in quick launcher</span>
          </label>
        </div>

        <div className="profiles-field">
          <label className="profiles-field-label">
            Prompt Template
            <span className="profiles-field-hint">Use {"{{key}}"} to auto-create inputs</span>
          </label>
          <textarea
            className="profiles-field-textarea"
            placeholder="Review the pull request at {{pr_url}}..."
            value={promptTemplate}
            onChange={(e) => setPromptTemplate(e.target.value)}
            rows={4}
          />
        </div>

        <div className="profiles-section">
          <div className="profiles-section-header">
            <span>Inputs</span>
            <button className="profiles-add-btn" onClick={handleAddInput}>+</button>
          </div>
          {inputs.length === 0 && (
            <span className="profiles-field-hint" style={{ padding: "4px 0" }}>
              No inputs. Use {"{{key}}"} in the template or add manually.
            </span>
          )}
          {inputs.map((input, i) => (
            <div key={i} className="profiles-input-row">
              <input
                className="profiles-field-input profiles-input-label"
                type="text"
                placeholder="Label"
                value={input.label}
                onChange={(e) => handleInputChange(i, "label", e.target.value)}
              />
              <select
                className="profiles-field-input profiles-input-type"
                value={input.input_type}
                onChange={(e) => handleInputChange(i, "input_type", e.target.value)}
              >
                <option value="text">Text</option>
                <option value="select">Select</option>
                <option value="list">List</option>
              </select>
              <button
                className="profiles-remove-btn"
                onClick={() => handleRemoveInput(i)}
              >
                &times;
              </button>
            </div>
          ))}
        </div>

        <div className="profiles-field">
          <label className="profiles-field-label">Working Directory</label>
          <div className="profiles-radio-group">
            <label className="profiles-radio">
              <input
                type="radio"
                checked={workingDirType === "prompt"}
                onChange={() => setWorkingDirType("prompt")}
              />
              Ask every time
            </label>
            <label className="profiles-radio">
              <input
                type="radio"
                checked={workingDirType === "fixed"}
                onChange={() => setWorkingDirType("fixed")}
              />
              Fixed path
            </label>
            {inputs.length > 0 && (
              <label className="profiles-radio">
                <input
                  type="radio"
                  checked={workingDirType === "from_input"}
                  onChange={() => setWorkingDirType("from_input")}
                />
                From input field
              </label>
            )}
          </div>
          {workingDirType === "fixed" && (
            <input
              className="profiles-field-input"
              type="text"
              placeholder="/path/to/directory"
              value={fixedPath}
              onChange={(e) => setFixedPath(e.target.value)}
            />
          )}
          {workingDirType === "from_input" && inputs.length > 0 && (
            <select
              className="profiles-field-input"
              value={fromInputKey}
              onChange={(e) => setFromInputKey(e.target.value)}
            >
              <option value="">Select input...</option>
              {inputs.map((inp) => (
                <option key={inp.key} value={inp.key}>{inp.label || inp.key}</option>
              ))}
            </select>
          )}
        </div>

        <div className="profiles-section">
          <div
            className="profiles-section-header profiles-section-toggle"
            onClick={() => setShowAdvanced(!showAdvanced)}
          >
            <span>Advanced</span>
            <span className={`profiles-chevron ${showAdvanced ? "" : "collapsed"}`}>&#9662;</span>
          </div>
          {showAdvanced && (
            <div className="profiles-advanced">
              <div className="profiles-field">
                <label className="profiles-field-label">Model</label>
                <select
                  className="profiles-field-input"
                  value={useCustomModel ? "custom" : model}
                  onChange={(e) => {
                    if (e.target.value === "custom") {
                      setUseCustomModel(true);
                      setModel("");
                    } else {
                      setUseCustomModel(false);
                      setModel(e.target.value);
                    }
                  }}
                >
                  <option value="">Default</option>
                  <option value="sonnet">Sonnet (Recommended)</option>
                  <option value="opus">Opus</option>
                  <option value="haiku">Haiku</option>
                  <option value="custom">Custom model ID...</option>
                </select>
                {useCustomModel && (
                  <input
                    className="profiles-field-input"
                    type="text"
                    placeholder="e.g. claude-sonnet-4-20250514"
                    value={model}
                    onChange={(e) => setModel(e.target.value)}
                    style={{ marginTop: "8px" }}
                  />
                )}
              </div>
              <div className="profiles-field">
                <label className="profiles-field-label">
                  Allowed Tools
                  <span className="profiles-field-hint">Empty = all tools</span>
                </label>
                <input
                  className="profiles-field-input"
                  type="text"
                  placeholder="Read, Grep, Glob, Bash, Edit, Write, Task..."
                  value={allowedTools}
                  onChange={(e) => setAllowedTools(e.target.value)}
                />
              </div>
              <div className="profiles-field">
                <label className="profiles-field-label">
                  System Prompt
                  <span className="profiles-field-hint">Appended via --append-system-prompt</span>
                </label>
                <SystemPromptPicker
                  selected={systemPromptFile}
                  onSelect={setSystemPromptFile}
                />
              </div>
              <div className="profiles-field">
                <label className="profiles-field-label">
                  Skills
                  <span className="profiles-field-hint">Select skills to activate when launching</span>
                </label>
                <SkillPicker
                  selectedSkills={selectedSkills}
                  onSelectionChange={setSelectedSkills}
                />
              </div>
            </div>
          )}
        </div>
      </div>
      <div className="profiles-editor-footer">
        <button className="profiles-cancel-btn" onClick={onClose}>
          Cancel
        </button>
        <button
          className="profiles-save-btn"
          onClick={handleSave}
          disabled={!name.trim()}
        >
          Save
        </button>
      </div>
    </div>
  );
}
