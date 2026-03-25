use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ConfigLayer {
    Default = 0,
    User = 1,
    Project = 2,
    Runtime = 3,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigValue {
    pub value: serde_json::Value,
    pub layer: ConfigLayer,
}

#[derive(Clone)]
pub struct Config {
    values: Arc<RwLock<HashMap<String, Vec<ConfigValue>>>>,
    schemas: Arc<RwLock<HashMap<String, serde_json::Value>>>,
    user_file_path: Arc<RwLock<Option<PathBuf>>>,
}

impl Config {
    pub fn new() -> Self {
        Self {
            values: Arc::new(RwLock::new(HashMap::new())),
            schemas: Arc::new(RwLock::new(HashMap::new())),
            user_file_path: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn set_user_file_path(&self, path: PathBuf) {
        *self.user_file_path.write().await = Some(path);
    }

    pub async fn load_from_file(&self, path: &PathBuf, layer: ConfigLayer) -> Result<(), ConfigError> {
        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| ConfigError::IoError(e.to_string()))?;

        let table: toml::Table = content
            .parse()
            .map_err(|e: toml::de::Error| ConfigError::ParseError(e.to_string()))?;

        self.merge_toml(&table, "", layer).await;
        debug!(?layer, path = %path.display(), "Loaded config");
        Ok(())
    }

    async fn merge_toml(&self, table: &toml::Table, prefix: &str, layer: ConfigLayer) {
        for (key, value) in table {
            let full_key = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{}.{}", prefix, key)
            };

            match value {
                toml::Value::Table(subtable) => {
                    Box::pin(self.merge_toml(subtable, &full_key, layer)).await;
                }
                _ => {
                    let json_value = toml_to_json(value);
                    self.set_value(&full_key, json_value, layer).await;
                }
            }
        }
    }

    pub async fn set_value(&self, key: &str, value: serde_json::Value, layer: ConfigLayer) {
        let mut values = self.values.write().await;
        let entry = values.entry(key.to_string()).or_insert_with(Vec::new);
        entry.retain(|v| v.layer != layer);
        entry.push(ConfigValue { value, layer });
        entry.sort_by_key(|v| v.layer);
    }

    pub async fn get(&self, key: &str) -> Option<serde_json::Value> {
        let values = self.values.read().await;
        values
            .get(key)
            .and_then(|entries| entries.last())
            .map(|v| v.value.clone())
    }

    pub async fn get_or_default(&self, key: &str, default: serde_json::Value) -> serde_json::Value {
        self.get(key).await.unwrap_or(default)
    }

    pub async fn get_string(&self, key: &str) -> Option<String> {
        self.get(key).await.and_then(|v| v.as_str().map(String::from))
    }

    pub async fn get_u64(&self, key: &str) -> Option<u64> {
        self.get(key).await.and_then(|v| v.as_u64())
    }

    pub async fn get_bool(&self, key: &str) -> Option<bool> {
        self.get(key).await.and_then(|v| v.as_bool())
    }

    pub async fn register_schema(&self, extension_id: &str, schema: serde_json::Value) {
        self.schemas
            .write()
            .await
            .insert(extension_id.to_string(), schema);
    }

    pub async fn all_keys(&self) -> Vec<String> {
        self.values.read().await.keys().cloned().collect()
    }

    /// Persist all User-layer values to the user config file as TOML.
    pub async fn save_user_config(&self) -> Result<(), ConfigError> {
        let path = self.user_file_path.read().await;
        let path = path.as_ref().ok_or_else(|| ConfigError::IoError("No user config file path set".to_string()))?;
        let path = path.clone();
        drop(path);

        let file_path = self.user_file_path.read().await.clone().unwrap();

        // Collect all User-layer values
        let values = self.values.read().await;
        let mut root = toml::Table::new();

        for (key, entries) in values.iter() {
            if let Some(cv) = entries.iter().find(|v| v.layer == ConfigLayer::User) {
                let toml_val = json_to_toml(&cv.value);
                // Support dotted keys: "autoFocus.tabAutoSwitch" -> [autoFocus] tabAutoSwitch
                let parts: Vec<&str> = key.split('.').collect();
                if parts.len() == 2 {
                    let section = root.entry(parts[0].to_string())
                        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
                    if let toml::Value::Table(t) = section {
                        t.insert(parts[1].to_string(), toml_val);
                    }
                } else {
                    root.insert(key.clone(), toml_val);
                }
            }
        }
        drop(values);

        let content = toml::to_string_pretty(&root)
            .map_err(|e| ConfigError::ParseError(e.to_string()))?;

        // Ensure parent directory exists
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| ConfigError::IoError(e.to_string()))?;
        }

        tokio::fs::write(&file_path, content).await
            .map_err(|e| ConfigError::IoError(e.to_string()))?;

        debug!(path = %file_path.display(), "Saved user config");
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    IoError(String),
    #[error("Parse error: {0}")]
    ParseError(String),
}

fn json_to_toml(value: &serde_json::Value) -> toml::Value {
    match value {
        serde_json::Value::String(s) => toml::Value::String(s.clone()),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                toml::Value::Integer(i)
            } else if let Some(f) = n.as_f64() {
                toml::Value::Float(f)
            } else {
                toml::Value::String(n.to_string())
            }
        }
        serde_json::Value::Bool(b) => toml::Value::Boolean(*b),
        serde_json::Value::Array(arr) => {
            toml::Value::Array(arr.iter().map(json_to_toml).collect())
        }
        serde_json::Value::Object(map) => {
            let table: toml::Table = map.iter().map(|(k, v)| (k.clone(), json_to_toml(v))).collect();
            toml::Value::Table(table)
        }
        serde_json::Value::Null => toml::Value::String(String::new()),
    }
}

fn toml_to_json(value: &toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::json!(*i),
        toml::Value::Float(f) => serde_json::json!(*f),
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Array(arr) => {
            serde_json::Value::Array(arr.iter().map(toml_to_json).collect())
        }
        toml::Value::Table(table) => {
            let map: serde_json::Map<String, serde_json::Value> = table
                .iter()
                .map(|(k, v)| (k.clone(), toml_to_json(v)))
                .collect();
            serde_json::Value::Object(map)
        }
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
    }
}
