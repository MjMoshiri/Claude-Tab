use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    pub name: String,
    pub source_path: String,
    pub is_active: bool,
    pub group: String,
}

fn derive_group_name(source_dir: &Path) -> String {
    let home = dirs_home();
    let default_source = home.join(".agents").join("skills");
    if source_dir == default_source {
        return "My Skills".to_string();
    }
    // Use the parent directory's name (e.g., `.../oway-skills/skills/` → "oway-skills")
    source_dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("Other")
        .to_string()
}

#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Skill not found: {0}")]
    NotFound(String),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl Serialize for SkillError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub struct SkillManager {
    lock: Mutex<()>,
}

impl SkillManager {
    pub fn new() -> Self {
        Self {
            lock: Mutex::new(()),
        }
    }

    fn skills_dir() -> PathBuf {
        dirs_home().join(".claude").join("skills")
    }

    /// Discover source directories by scanning:
    /// 1. ~/.agents/skills/ (default source)
    /// 2. Existing symlinks in ~/.claude/skills/ — resolve their parent dirs
    pub fn discover_source_dirs() -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        let mut seen = HashSet::new();

        // Default source
        let default_source = dirs_home().join(".agents").join("skills");
        if default_source.is_dir() {
            dirs.push(default_source.clone());
            seen.insert(default_source);
        }

        // Auto-discover from existing symlinks
        let skills_dir = Self::skills_dir();
        if skills_dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&skills_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_symlink() {
                        if let Ok(target) = std::fs::read_link(&path) {
                            // Resolve to absolute if relative
                            let resolved = if target.is_absolute() {
                                target
                            } else {
                                skills_dir.join(&target)
                            };
                            if let Some(parent) = resolved.parent() {
                                let parent_buf = parent.to_path_buf();
                                if parent_buf.is_dir() && !seen.contains(&parent_buf) {
                                    seen.insert(parent_buf.clone());
                                    dirs.push(parent_buf);
                                }
                            }
                        }
                    }
                }
            }
        }

        dirs
    }

    /// List all available skills from all source directories.
    /// Each skill's `is_active` flag is set based on whether a symlink
    /// exists for it in `~/.claude/skills/`.
    pub fn list_available_skills(&self) -> Result<Vec<SkillInfo>, SkillError> {
        let source_dirs = Self::discover_source_dirs();
        let skills_dir = Self::skills_dir();
        let mut skills = HashMap::new();

        // Scan active symlinks first
        let active_names: HashSet<String> = if skills_dir.is_dir() {
            std::fs::read_dir(&skills_dir)?
                .flatten()
                .filter(|e| e.path().is_symlink() || e.path().is_dir())
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .collect()
        } else {
            HashSet::new()
        };

        // Scan all source dirs
        for source_dir in &source_dirs {
            let group = derive_group_name(source_dir);
            if let Ok(entries) = std::fs::read_dir(source_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    // Only include directories (skills are dirs)
                    if entry.path().is_dir() && !name.starts_with('.') {
                        if !skills.contains_key(&name) {
                            skills.insert(name.clone(), SkillInfo {
                                name: name.clone(),
                                source_path: entry.path().to_string_lossy().to_string(),
                                is_active: active_names.contains(&name),
                                group: group.clone(),
                            });
                        }
                    }
                }
            }
        }

        let mut result: Vec<_> = skills.into_values().collect();
        result.sort_by(|a, b| a.group.cmp(&b.group).then(a.name.cmp(&b.name)));
        Ok(result)
    }

    /// Synchronize skills: remove symlinks not in the given list,
    /// add symlinks for skills that are in the list.
    pub fn sync_skills(&self, skill_names: &[String]) -> Result<(), SkillError> {
        let _guard = self.lock.lock().map_err(|e| SkillError::Internal(e.to_string()))?;

        let skills_dir = Self::skills_dir();
        std::fs::create_dir_all(&skills_dir)?;

        let desired: HashSet<&str> = skill_names.iter().map(|s| s.as_str()).collect();

        // Build a map of skill_name -> source_path from all source dirs
        let source_dirs = Self::discover_source_dirs();
        let mut source_map: HashMap<String, PathBuf> = HashMap::new();
        for source_dir in &source_dirs {
            if let Ok(entries) = std::fs::read_dir(source_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if entry.path().is_dir() && !name.starts_with('.') {
                        source_map.entry(name).or_insert_with(|| entry.path());
                    }
                }
            }
        }

        // Remove symlinks not in desired set
        if let Ok(entries) = std::fs::read_dir(&skills_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                if path.is_symlink() && !desired.contains(name.as_str()) {
                    debug!(skill = %name, "Removing skill symlink");
                    std::fs::remove_file(&path)?;
                }
            }
        }

        // Add symlinks for desired skills
        for skill_name in &desired {
            let link_path = skills_dir.join(skill_name);
            if link_path.exists() || link_path.is_symlink() {
                continue; // Already exists
            }
            if let Some(source_path) = source_map.get(*skill_name) {
                debug!(skill = %skill_name, source = %source_path.display(), "Creating skill symlink");
                #[cfg(unix)]
                std::os::unix::fs::symlink(source_path, &link_path)?;
                #[cfg(not(unix))]
                {
                    warn!(skill = %skill_name, "Symlinks not supported on this platform");
                }
            } else {
                warn!(skill = %skill_name, "Skill source not found, skipping");
            }
        }

        info!(count = skill_names.len(), "Skills synchronized");
        Ok(())
    }
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .expect("HOME environment variable must be set")
}
