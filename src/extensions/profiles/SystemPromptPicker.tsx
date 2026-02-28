import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { SystemPromptEntry } from "../../types/profile";

interface SystemPromptPickerProps {
  selected: string | null;
  onSelect: (name: string | null) => void;
}

export function SystemPromptPicker({ selected, onSelect }: SystemPromptPickerProps) {
  const [prompts, setPrompts] = useState<SystemPromptEntry[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let mounted = true;
    invoke<SystemPromptEntry[]>("list_system_prompts")
      .then((result) => {
        if (mounted) setPrompts(result);
      })
      .catch((err) => console.error("[SystemPromptPicker] Failed to load:", err))
      .finally(() => {
        if (mounted) setLoading(false);
      });
    return () => {
      mounted = false;
    };
  }, []);

  if (loading) {
    return <span className="profiles-field-hint">Loading system prompts...</span>;
  }

  if (prompts.length === 0) {
    return (
      <span className="profiles-field-hint">
        Add .md files to ~/.claude-tabs/system-prompts/
      </span>
    );
  }

  const selectedEntry = prompts.find((p) => p.name === selected);

  return (
    <div className="system-prompt-picker">
      <select
        className="system-prompt-picker-select"
        value={selected || ""}
        onChange={(e) => onSelect(e.target.value || null)}
      >
        <option value="">None</option>
        {prompts.map((p) => (
          <option key={p.name} value={p.name}>
            {p.name}
          </option>
        ))}
      </select>
      {selectedEntry && (
        <div className="system-prompt-picker-preview">{selectedEntry.preview}</div>
      )}
    </div>
  );
}
