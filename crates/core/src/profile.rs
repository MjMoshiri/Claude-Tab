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
pub struct McpConfig {
    #[serde(default)]
    pub config_path: Option<String>,
    #[serde(default)]
    pub servers: Option<serde_json::Value>,
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
    pub mcp_servers: Option<McpConfig>,
    #[serde(default)]
    pub skills: Option<Vec<String>>,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub disabled_mcps: Option<Vec<String>>,
    #[serde(default)]
    pub system_prompt_file: Option<String>,
    #[serde(default)]
    pub inputs: Vec<ProfileInput>,
    #[serde(default)]
    pub tags: Vec<String>,
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

    pub fn write_temp_mcp_config(
        &self,
        session_id: &str,
        mcp_config: &McpConfig,
    ) -> Result<Option<PathBuf>, String> {
        if let Some(ref servers) = mcp_config.servers {
            let tmp_dir = dirs_home().join(".claude-tabs").join("tmp");
            std::fs::create_dir_all(&tmp_dir)
                .map_err(|e| format!("Failed to create tmp dir: {}", e))?;
            let path = tmp_dir.join(format!("mcp-{}.json", session_id));
            let config_json = serde_json::json!({ "mcpServers": servers });
            let json = serde_json::to_string_pretty(&config_json)
                .map_err(|e| format!("Failed to serialize MCP config: {}", e))?;
            std::fs::write(&path, json)
                .map_err(|e| format!("Failed to write temp MCP config: {}", e))?;
            debug!(path = %path.display(), "Wrote temp MCP config");
            Ok(Some(path))
        } else if let Some(ref config_path) = mcp_config.config_path {
            Ok(Some(PathBuf::from(config_path)))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntry {
    pub name: String,
    pub server_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemPromptEntry {
    pub name: String,
    pub preview: String,
}

/// List MCP servers from all sources:
/// 1. ~/.claude/.mcp.json (user-configured servers)
/// 2. ~/.claude.json mcpServers (user-level)
/// 3. ~/.claude.json projects.{path}.mcpServers (project-level)
/// 4. Enabled plugins that contain .mcp.json (plugin-provided servers)
pub fn list_mcp_servers() -> Vec<McpServerEntry> {
    let home = dirs_home();
    let mut seen = std::collections::HashSet::new();
    let mut entries = Vec::new();

    // 1. User-configured MCP servers from ~/.claude/.mcp.json
    if let Some(servers) = read_mcp_json_servers(&home.join(".claude").join(".mcp.json")) {
        for (name, config) in &servers {
            if seen.insert(name.clone()) {
                entries.push(McpServerEntry {
                    name: name.clone(),
                    server_type: detect_server_type(config),
                });
            }
        }
    }

    // 2. User-level MCPs from ~/.claude.json -> mcpServers
    if let Some(servers) = read_claude_json_user_mcps(&home) {
        for (name, config) in &servers {
            if seen.insert(name.clone()) {
                entries.push(McpServerEntry {
                    name: name.clone(),
                    server_type: detect_server_type(config),
                });
            }
        }
    }

    // 3. Project-level MCPs from ~/.claude.json -> projects.{path}.mcpServers
    for (project_path, servers) in read_claude_json_project_mcps(&home) {
        let short_project = project_path
            .rsplit('/')
            .next()
            .unwrap_or(&project_path);
        for (name, _config) in &servers {
            if seen.insert(name.clone()) {
                entries.push(McpServerEntry {
                    name: name.clone(),
                    server_type: format!("project ({})", short_project),
                });
            }
        }
    }

    // 4. Plugin-provided MCP servers
    for (plugin_name, mcp_servers) in discover_plugin_mcps(&home) {
        for (server_name, _config) in &mcp_servers {
            if seen.insert(server_name.clone()) {
                entries.push(McpServerEntry {
                    name: server_name.clone(),
                    server_type: format!("plugin ({})", plugin_name),
                });
            }
        }
    }

    entries
}

fn detect_server_type(config: &serde_json::Value) -> String {
    if config.get("command").is_some() {
        "command".to_string()
    } else if let Some(t) = config.get("type").and_then(|v| v.as_str()) {
        t.to_string()
    } else {
        "unknown".to_string()
    }
}

/// Read mcpServers from a .mcp.json file. Returns None on any failure.
fn read_mcp_json_servers(
    path: &Path,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let content = std::fs::read_to_string(path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    // Support both top-level { "mcpServers": {...} } and flat { "name": {...} }
    if let Some(obj) = parsed.get("mcpServers").and_then(|v| v.as_object()) {
        Some(obj.clone())
    } else if let Some(obj) = parsed.as_object() {
        // Flat format used by plugin .mcp.json files (e.g. { "context7": { "command": "npx", ... } })
        Some(obj.clone())
    } else {
        None
    }
}

/// Read user-level mcpServers from ~/.claude.json top-level.
fn read_claude_json_user_mcps(
    home: &Path,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let path = home.join(".claude.json");
    let content = std::fs::read_to_string(&path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    let servers = parsed.get("mcpServers")?.as_object()?;
    if servers.is_empty() {
        return None;
    }
    Some(servers.clone())
}

/// Read project-level mcpServers from ~/.claude.json -> projects.{path}.mcpServers.
/// Returns Vec<(project_path, servers_map)>.
fn read_claude_json_project_mcps(
    home: &Path,
) -> Vec<(String, serde_json::Map<String, serde_json::Value>)> {
    let path = home.join(".claude.json");
    let mut results = Vec::new();
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return results,
    };
    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return results,
    };
    let projects = match parsed.get("projects").and_then(|v| v.as_object()) {
        Some(p) => p,
        None => return results,
    };
    for (project_path, project_val) in projects {
        if let Some(servers) = project_val.get("mcpServers").and_then(|v| v.as_object()) {
            if !servers.is_empty() {
                results.push((project_path.clone(), servers.clone()));
            }
        }
    }
    results
}

/// Collect all MCP server configs from all sources into a single map.
/// If `working_directory` is provided, project-level MCPs are included only for
/// projects whose path is a prefix of (or equal to) the working directory.
fn collect_all_mcp_configs(
    working_directory: Option<&str>,
) -> serde_json::Map<String, serde_json::Value> {
    let home = dirs_home();
    let mut all = serde_json::Map::new();

    // 1. ~/.claude/.mcp.json
    if let Some(servers) = read_mcp_json_servers(&home.join(".claude").join(".mcp.json")) {
        for (name, config) in servers {
            all.insert(name, config);
        }
    }

    // 2. User-level MCPs from ~/.claude.json
    if let Some(servers) = read_claude_json_user_mcps(&home) {
        for (name, config) in servers {
            all.entry(name).or_insert(config);
        }
    }

    // 3. Project-level MCPs from ~/.claude.json
    for (project_path, servers) in read_claude_json_project_mcps(&home) {
        let include = match working_directory {
            Some(wd) => wd.starts_with(&project_path) || project_path.starts_with(wd),
            None => true, // Include all when no working dir filter
        };
        if include {
            for (name, config) in servers {
                all.entry(name).or_insert(config);
            }
        }
    }

    // 4. Plugin MCPs
    for (_plugin_name, servers) in discover_plugin_mcps(&home) {
        for (name, config) in servers {
            all.entry(name).or_insert(config);
        }
    }

    all
}

/// Discover MCP servers provided by enabled plugins.
/// Returns Vec<(plugin_short_name, mcp_servers_map)>.
fn discover_plugin_mcps(
    home: &Path,
) -> Vec<(String, serde_json::Map<String, serde_json::Value>)> {
    let claude_dir = home.join(".claude");
    let mut results = Vec::new();

    // Read enabledPlugins from settings.json
    let settings_path = claude_dir.join("settings.json");
    let enabled = match read_enabled_plugins(&settings_path) {
        Some(e) => e,
        None => return results,
    };

    // Read installed_plugins.json for install paths
    let installed_path = claude_dir.join("plugins").join("installed_plugins.json");
    let installed: serde_json::Value = match std::fs::read_to_string(&installed_path)
        .ok()
        .and_then(|c| serde_json::from_str(&c).ok())
    {
        Some(v) => v,
        None => return results,
    };

    let plugins_map = match installed.get("plugins").and_then(|v| v.as_object()) {
        Some(m) => m,
        None => return results,
    };

    for (full_name, is_enabled) in &enabled {
        if !is_enabled {
            continue;
        }
        // Extract short name: "context7@claude-plugins-official" -> "context7"
        let short_name = full_name.split('@').next().unwrap_or(full_name);

        // Find install path from installed_plugins.json
        if let Some(versions) = plugins_map.get(full_name).and_then(|v| v.as_array()) {
            if let Some(latest) = versions.first() {
                if let Some(install_path) = latest.get("installPath").and_then(|v| v.as_str()) {
                    let mcp_path = Path::new(install_path).join(".mcp.json");
                    if let Some(servers) = read_mcp_json_servers(&mcp_path) {
                        if !servers.is_empty() {
                            results.push((short_name.to_string(), servers));
                        }
                    }
                }
            }
        }
    }

    results
}

/// Read enabledPlugins from settings.json. Returns map of plugin_name -> enabled.
fn read_enabled_plugins(path: &Path) -> Option<HashMap<String, bool>> {
    let content = std::fs::read_to_string(path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    let plugins = parsed.get("enabledPlugins")?.as_object()?;
    let mut map = HashMap::new();
    for (name, val) in plugins {
        map.insert(name.clone(), val.as_bool().unwrap_or(false));
    }
    Some(map)
}

/// Write a filtered MCP config excluding disabled servers, optionally merging extra servers.
/// Collects MCPs from all sources (`.mcp.json`, `~/.claude.json` user & project level, plugins),
/// filters out disabled ones, and merges any extra profile-specific servers.
pub fn write_filtered_mcp_config(
    session_id: &str,
    disabled_mcps: &[String],
    extra_mcp: Option<&McpConfig>,
    working_directory: Option<&str>,
) -> Result<Option<PathBuf>, String> {
    let all_servers = collect_all_mcp_configs(working_directory);

    // Filter out disabled servers and explicitly enable the rest
    let mut final_servers = serde_json::Map::new();
    for (name, config) in all_servers {
        if !disabled_mcps.contains(&name) {
            let mut server_config = config;
            // Ensure each server is explicitly enabled so Claude Code auto-connects them
            if let Some(obj) = server_config.as_object_mut() {
                obj.insert("disabled".to_string(), serde_json::Value::Bool(false));
            }
            final_servers.insert(name, server_config);
        }
    }

    // Merge extra servers from profile
    if let Some(mcp) = extra_mcp {
        if let Some(ref servers_val) = mcp.servers {
            if let Some(obj) = servers_val.as_object() {
                for (name, config) in obj {
                    final_servers.insert(name.clone(), config.clone());
                }
            }
        }
    }

    if final_servers.is_empty() {
        return Ok(None);
    }

    let tmp_dir = dirs_home().join(".claude-tabs").join("tmp");
    std::fs::create_dir_all(&tmp_dir)
        .map_err(|e| format!("Failed to create tmp dir: {}", e))?;
    let path = tmp_dir.join(format!("mcp-{}.json", session_id));
    let config_json = serde_json::json!({ "mcpServers": final_servers });
    let json = serde_json::to_string_pretty(&config_json)
        .map_err(|e| format!("Failed to serialize filtered MCP config: {}", e))?;
    std::fs::write(&path, &json)
        .map_err(|e| format!("Failed to write filtered MCP config: {}", e))?;
    debug!(path = %path.display(), "Wrote filtered MCP config");
    Ok(Some(path))
}

/// Update the disabledMcpServers list for a project in ~/.claude.json.
/// This is the source of truth Claude Code uses for MCP enable/disable state.
pub fn set_project_disabled_mcps(
    working_directory: &str,
    disabled_mcps: &[String],
) -> Result<(), String> {
    let path = dirs_home().join(".claude.json");
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read ~/.claude.json: {}", e))?;
    let mut root: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse ~/.claude.json: {}", e))?;

    let projects = root
        .as_object_mut()
        .ok_or("~/.claude.json is not an object")?
        .entry("projects")
        .or_insert_with(|| serde_json::json!({}));

    let project = projects
        .as_object_mut()
        .ok_or("projects is not an object")?
        .entry(working_directory)
        .or_insert_with(|| serde_json::json!({}));

    let disabled_arr: Vec<serde_json::Value> = disabled_mcps
        .iter()
        .map(|s| serde_json::Value::String(s.clone()))
        .collect();

    project
        .as_object_mut()
        .ok_or("project entry is not an object")?
        .insert(
            "disabledMcpServers".to_string(),
            serde_json::Value::Array(disabled_arr),
        );

    let json = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("Failed to serialize ~/.claude.json: {}", e))?;
    std::fs::write(&path, json)
        .map_err(|e| format!("Failed to write ~/.claude.json: {}", e))?;

    debug!(
        working_directory = %working_directory,
        disabled = ?disabled_mcps,
        "Updated project disabledMcpServers"
    );
    Ok(())
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

pub fn cleanup_temp_mcp_config(session_id: &str) {
    let path = dirs_home()
        .join(".claude-tabs")
        .join("tmp")
        .join(format!("mcp-{}.json", session_id));
    if path.exists() {
        if let Err(e) = std::fs::remove_file(&path) {
            warn!(error = %e, path = %path.display(), "Failed to cleanup temp MCP config");
        } else {
            debug!(path = %path.display(), "Cleaned up temp MCP config");
        }
    }
}
