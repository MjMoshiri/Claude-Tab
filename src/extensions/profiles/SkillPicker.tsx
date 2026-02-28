import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { SkillInfo } from "../../types/profile";
import { useConfig } from "../../kernel/ConfigProvider";

interface SkillPickerProps {
  selectedSkills: Set<string>;
  onSelectionChange: (skills: Set<string>) => void;
}

type SkillGroups = Record<string, string[]>;

export function SkillPicker({ selectedSkills, onSelectionChange }: SkillPickerProps) {
  const [skills, setSkills] = useState<SkillInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});
  const [initialized, setInitialized] = useState(false);
  const config = useConfig();
  const userGroups: SkillGroups = config.get<SkillGroups>("skillGroups", {});

  useEffect(() => {
    let mounted = true;
    invoke<SkillInfo[]>("list_available_skills")
      .then((result) => {
        if (mounted) setSkills(result);
      })
      .catch((err) => console.error("[SkillPicker] Failed to load skills:", err))
      .finally(() => { if (mounted) setLoading(false); });
    return () => { mounted = false; };
  }, []);

  // Initialize all sections as collapsed
  const groupNames = Object.keys(userGroups).sort();
  useEffect(() => {
    if (initialized || skills.length === 0) return;
    const initial: Record<string, boolean> = { _all: true };
    for (const name of groupNames) {
      initial[name] = true;
    }
    setCollapsed(initial);
    setInitialized(true);
  }, [skills, groupNames, initialized]);

  const skillNames = new Set(skills.map((s) => s.name));

  // Build group entries: only include skills that actually exist
  const resolvedGroups = groupNames
    .map((name) => ({
      name,
      skills: (userGroups[name] || []).filter((s) => skillNames.has(s)),
    }))
    .filter((g) => g.skills.length > 0);

  const toggleSkill = useCallback((name: string) => {
    const next = new Set(selectedSkills);
    if (next.has(name)) {
      next.delete(name);
    } else {
      next.add(name);
    }
    onSelectionChange(next);
  }, [selectedSkills, onSelectionChange]);

  const toggleGroup = useCallback((groupSkillNames: string[]) => {
    const allSelected = groupSkillNames.every((s) => selectedSkills.has(s));
    const next = new Set(selectedSkills);
    for (const s of groupSkillNames) {
      if (allSelected) {
        next.delete(s);
      } else {
        next.add(s);
      }
    }
    onSelectionChange(next);
  }, [selectedSkills, onSelectionChange]);

  const toggleCollapse = useCallback((key: string) => {
    setCollapsed((prev) => ({ ...prev, [key]: !prev[key] }));
  }, []);

  const toggleAll = useCallback(() => {
    const allSelected = skills.every((s) => selectedSkills.has(s.name));
    const next = new Set(selectedSkills);
    for (const s of skills) {
      if (allSelected) {
        next.delete(s.name);
      } else {
        next.add(s.name);
      }
    }
    onSelectionChange(next);
  }, [selectedSkills, onSelectionChange, skills]);

  if (loading) {
    return <span className="profiles-field-hint">Loading skills...</span>;
  }

  if (skills.length === 0) {
    return <span className="profiles-field-hint">No skills found in ~/.agents/skills/</span>;
  }

  const allSelectedCount = skills.filter((s) => selectedSkills.has(s.name)).length;
  const allAllSelected = allSelectedCount === skills.length;
  const allSomeSelected = allSelectedCount > 0 && !allAllSelected;

  return (
    <div className="skill-picker">
      {/* User-defined groups for quick batch selection */}
      {resolvedGroups.map((group) => {
        const selectedCount = group.skills.filter((s) => selectedSkills.has(s)).length;
        const allSelected = selectedCount === group.skills.length;
        const someSelected = selectedCount > 0 && !allSelected;
        const isCollapsed = collapsed[group.name] !== false;

        return (
          <div key={group.name} className="skill-picker-group">
            <div className="skill-picker-group-header">
              <GroupCheckbox
                checked={allSelected}
                indeterminate={someSelected}
                onChange={() => toggleGroup(group.skills)}
              />
              <span
                className="skill-picker-group-name"
                onClick={() => toggleCollapse(group.name)}
              >
                {group.name}
              </span>
              <span className="skill-picker-group-count">
                {selectedCount}/{group.skills.length}
              </span>
              <span
                className={`skill-picker-chevron ${isCollapsed ? "collapsed" : ""}`}
                onClick={() => toggleCollapse(group.name)}
              >
                &#9662;
              </span>
            </div>
            {!isCollapsed && (
              <div className="skill-picker-group-items">
                {group.skills.map((skillName) => (
                  <label key={skillName} className="skill-picker-item">
                    <input
                      type="checkbox"
                      checked={selectedSkills.has(skillName)}
                      onChange={() => toggleSkill(skillName)}
                    />
                    <span>{skillName}</span>
                  </label>
                ))}
              </div>
            )}
          </div>
        );
      })}

      {/* All skills section */}
      <div className="skill-picker-group">
        <div className="skill-picker-group-header">
          <GroupCheckbox
            checked={allAllSelected}
            indeterminate={allSomeSelected}
            onChange={toggleAll}
          />
          <span
            className="skill-picker-group-name"
            onClick={() => toggleCollapse("_all")}
          >
            All Skills
          </span>
          <span className="skill-picker-group-count">
            {allSelectedCount}/{skills.length}
          </span>
          <span
            className={`skill-picker-chevron ${collapsed["_all"] !== false ? "collapsed" : ""}`}
            onClick={() => toggleCollapse("_all")}
          >
            &#9662;
          </span>
        </div>
        {collapsed["_all"] === false && (
          <div className="skill-picker-group-items">
            {skills.map((skill) => (
              <label key={skill.name} className="skill-picker-item">
                <input
                  type="checkbox"
                  checked={selectedSkills.has(skill.name)}
                  onChange={() => toggleSkill(skill.name)}
                />
                <span>{skill.name}</span>
              </label>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function GroupCheckbox({
  checked,
  indeterminate,
  onChange,
}: {
  checked: boolean;
  indeterminate: boolean;
  onChange: () => void;
}) {
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (ref.current) {
      ref.current.indeterminate = indeterminate;
    }
  }, [indeterminate]);

  return (
    <input
      ref={ref}
      type="checkbox"
      checked={checked}
      onChange={onChange}
    />
  );
}
