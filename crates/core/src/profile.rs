use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileInput {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default = "default_input_type")]
    pub input_type: String,
    #[serde(default = "default_true")]
    pub required: bool,
    #[serde(default)]
    pub options: Option<Vec<String>>,
    #[serde(default)]
    pub default: Option<String>,
}

fn default_input_type() -> String {
    "text".to_string()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkingDirConfig {
    Fixed { path: String },
    Prompt,
    FromInput { key: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub working_directory: Option<WorkingDirConfig>,
    #[serde(default)]
    pub prompt_template: Option<String>,
    #[serde(default)]
    pub auto_execute: bool,
    #[serde(default)]
    pub skills: Option<Vec<String>>,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub system_prompt_file: Option<String>,
    #[serde(default)]
    pub inputs: Vec<ProfileInput>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub dangerously_skip_permissions: bool,
    #[serde(default)]
    pub auto_accept_policy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pack {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub profile_ids: Vec<String>,
}

fn default_version() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileLaunchRequest {
    pub profile_id: String,
    #[serde(default)]
    pub input_values: HashMap<String, String>,
    #[serde(default)]
    pub working_directory: Option<String>,
}

pub struct ProfileStore {
    global_dir: PathBuf,
    profiles: RwLock<HashMap<String, Profile>>,
}

impl ProfileStore {
    pub fn new() -> Self {
        let home = dirs_home();
        let global_dir = home.join(".claude-tabs").join("profiles");
        Self {
            global_dir,
            profiles: RwLock::new(HashMap::new()),
        }
    }

    pub async fn init(&self) {
        if let Err(e) = std::fs::create_dir_all(&self.global_dir) {
            error!(error = %e, "Failed to create profiles directory");
            return;
        }
        // Ensure system-prompts directory exists
        let prompts_dir = dirs_home().join(".claude-tabs").join("system-prompts");
        if let Err(e) = std::fs::create_dir_all(&prompts_dir) {
            warn!(error = %e, "Failed to create system-prompts directory");
        }
        self.reload().await;
    }

    pub async fn reload(&self) {
        let mut profiles = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(&self.global_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "json") {
                    match load_profile_file(&path) {
                        Ok(profile) => {
                            debug!(id = %profile.id, "Loaded profile");
                            profiles.insert(profile.id.clone(), profile);
                        }
                        Err(e) => {
                            warn!(path = %path.display(), error = %e, "Failed to load profile");
                        }
                    }
                }
            }
        }
        info!(count = profiles.len(), "Profiles loaded");
        *self.profiles.write().await = profiles;
    }

    pub async fn list(&self) -> Vec<Profile> {
        let profiles = self.profiles.read().await;
        let mut list: Vec<_> = profiles.values().cloned().collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    pub async fn get(&self, id: &str) -> Option<Profile> {
        self.profiles.read().await.get(id).cloned()
    }

    pub async fn save(&self, profile: Profile) -> Result<(), String> {
        let filename = format!("{}.json", &profile.id);
        let path = self.global_dir.join(&filename);
        let json = serde_json::to_string_pretty(&profile)
            .map_err(|e| format!("Failed to serialize profile: {}", e))?;
        std::fs::write(&path, json)
            .map_err(|e| format!("Failed to write profile file: {}", e))?;
        info!(id = %profile.id, path = %path.display(), "Profile saved");
        self.profiles.write().await.insert(profile.id.clone(), profile);
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<(), String> {
        let filename = format!("{}.json", id);
        let path = self.global_dir.join(&filename);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("Failed to delete profile file: {}", e))?;
        }
        self.profiles.write().await.remove(id);
        info!(id = %id, "Profile deleted");
        Ok(())
    }

    pub fn resolve_prompt(&self, template: &str, input_values: &HashMap<String, String>) -> String {
        let mut result = template.to_string();
        for (key, value) in input_values {
            result = result.replace(&format!("{{{{{}}}}}", key), value);
        }
        result
    }

}

pub struct PackStore {
    dir: PathBuf,
    packs: RwLock<HashMap<String, Pack>>,
}

impl PackStore {
    pub fn new() -> Self {
        let home = dirs_home();
        let dir = home.join(".claude-tabs").join("packs");
        Self {
            dir,
            packs: RwLock::new(HashMap::new()),
        }
    }

    pub async fn init(&self) {
        if let Err(e) = std::fs::create_dir_all(&self.dir) {
            error!(error = %e, "Failed to create packs directory");
            return;
        }
        self.reload().await;
    }

    pub async fn reload(&self) {
        let mut packs = HashMap::new();
        if let Ok(entries) = std::fs::read_dir(&self.dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |ext| ext == "json") {
                    match load_pack_file(&path) {
                        Ok(pack) => {
                            debug!(id = %pack.id, "Loaded pack");
                            packs.insert(pack.id.clone(), pack);
                        }
                        Err(e) => {
                            warn!(path = %path.display(), error = %e, "Failed to load pack");
                        }
                    }
                }
            }
        }
        info!(count = packs.len(), "Packs loaded");
        *self.packs.write().await = packs;
    }

    pub async fn list(&self) -> Vec<Pack> {
        let packs = self.packs.read().await;
        let mut list: Vec<_> = packs.values().cloned().collect();
        list.sort_by(|a, b| a.name.cmp(&b.name));
        list
    }

    pub async fn get(&self, id: &str) -> Option<Pack> {
        self.packs.read().await.get(id).cloned()
    }

    pub async fn save(&self, pack: Pack) -> Result<(), String> {
        let filename = format!("{}.json", &pack.id);
        let path = self.dir.join(&filename);
        let json = serde_json::to_string_pretty(&pack)
            .map_err(|e| format!("Failed to serialize pack: {}", e))?;
        std::fs::write(&path, json)
            .map_err(|e| format!("Failed to write pack file: {}", e))?;
        info!(id = %pack.id, path = %path.display(), "Pack saved");
        self.packs.write().await.insert(pack.id.clone(), pack);
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> Result<(), String> {
        let filename = format!("{}.json", id);
        let path = self.dir.join(&filename);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("Failed to delete pack file: {}", e))?;
        }
        self.packs.write().await.remove(id);
        info!(id = %id, "Pack deleted");
        Ok(())
    }
}

fn load_pack_file(path: &Path) -> Result<Pack, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemPromptEntry {
    pub name: String,
    pub preview: String,
}

/// List system prompt files from ~/.claude-tabs/system-prompts/
pub fn list_system_prompts() -> Vec<SystemPromptEntry> {
    let dir = dirs_home().join(".claude-tabs").join("system-prompts");
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut prompts = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "md") {
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let preview = std::fs::read_to_string(&path)
                .unwrap_or_default()
                .chars()
                .take(100)
                .collect::<String>();
            prompts.push(SystemPromptEntry { name, preview });
        }
    }
    prompts.sort_by(|a, b| a.name.cmp(&b.name));
    prompts
}

/// Read full content of a system prompt file
pub fn read_system_prompt_content(name: &str) -> Result<String, String> {
    let path = dirs_home()
        .join(".claude-tabs")
        .join("system-prompts")
        .join(format!("{}.md", name));
    std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read system prompt '{}': {}", name, e))
}

/// Save a system prompt file to ~/.claude-tabs/system-prompts/{name}.md
pub fn save_system_prompt(name: &str, content: &str) -> Result<(), String> {
    let dir = dirs_home().join(".claude-tabs").join("system-prompts");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Failed to create system-prompts dir: {}", e))?;
    let path = dir.join(format!("{}.md", name));
    std::fs::write(&path, content)
        .map_err(|e| format!("Failed to write system prompt '{}': {}", name, e))?;
    info!(name = %name, "Saved system prompt");
    Ok(())
}

/// Delete a system prompt file
pub fn delete_system_prompt(name: &str) -> Result<(), String> {
    let path = dirs_home()
        .join(".claude-tabs")
        .join("system-prompts")
        .join(format!("{}.md", name));
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("Failed to delete system prompt '{}': {}", name, e))?;
        info!(name = %name, "Deleted system prompt");
    }
    Ok(())
}

fn load_profile_file(path: &Path) -> Result<Profile, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .expect("HOME environment variable must be set")
}
