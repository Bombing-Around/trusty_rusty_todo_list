use crate::models::StorageError;
use crate::storage::{config::ConfigStorage, json::JsonStorage, sqlite, Storage};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Migration error: {0}")]
    #[allow(dead_code)]
    Migration(String),
    #[error("Invalid key: {0}")]
    InvalidKey(String),
}

impl From<StorageError> for ConfigError {
    fn from(error: StorageError) -> Self {
        ConfigError::Storage(error.to_string())
    }
}

const VALID_STORAGE_TYPES: &[&str] = &["json", "sqlite"];
const VALID_PRIORITIES: &[&str] = &["high", "medium", "low"];

fn validate_storage_path(path: &str) -> Result<PathBuf, ConfigError> {
    // Check for null bytes and other invalid characters
    if path.contains('\0') {
        return Err(ConfigError::InvalidConfig(
            "Path contains invalid characters".to_string(),
        ));
    }

    let path = shellexpand::tilde(path);
    let path = PathBuf::from(path.as_ref());

    // Check if parent directory exists
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            return Err(ConfigError::InvalidConfig(format!(
                "Parent directory does not exist: {}",
                parent.display()
            )));
        }

        // Check if directory is writable
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let Ok(metadata) = parent.metadata() {
                if metadata.mode() & 0o200 == 0 {
                    return Err(ConfigError::InvalidConfig(format!(
                        "Directory is not writable: {}",
                        parent.display()
                    )));
                }
            }
        }
    }

    // Additional path validation
    if path.as_os_str().is_empty() {
        return Err(ConfigError::InvalidConfig(
            "Path cannot be empty".to_string(),
        ));
    }

    Ok(path)
}

fn validate_storage_type(value: &str) -> Result<(), ConfigError> {
    if !VALID_STORAGE_TYPES.contains(&value) {
        return Err(ConfigError::InvalidConfig(format!(
            "storage.type must be one of: {}",
            VALID_STORAGE_TYPES.join(", ")
        )));
    }
    Ok(())
}

fn validate_priority(value: &str) -> Result<(), ConfigError> {
    if !VALID_PRIORITIES.contains(&value) {
        return Err(ConfigError::InvalidConfig(format!(
            "priority must be one of: {}",
            VALID_PRIORITIES.join(", ")
        )));
    }
    Ok(())
}

fn validate_lifespan(value: &str) -> Result<u32, ConfigError> {
    value.parse().map_err(|_| {
        ConfigError::InvalidConfig(
            "deleted-task-lifespan must be a positive integer or 0".to_string(),
        )
    })
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Config {
    #[serde(default = "default_deleted_task_lifespan")]
    pub deleted_task_lifespan: Option<u32>,
    #[serde(default = "default_storage_type")]
    pub storage_type: Option<String>,
    #[serde(default = "default_storage_path")]
    pub storage_path: Option<String>,
    #[serde(default)]
    pub default_category: Option<String>,
    #[serde(default = "default_priority")]
    pub default_priority: Option<String>,
}

impl Config {
    fn validate(&self) -> Result<(), ConfigError> {
        if let Some(ref storage_type) = self.storage_type {
            validate_storage_type(storage_type)?;
        }
        if let Some(ref priority) = self.default_priority {
            validate_priority(priority)?;
        }
        if let Some(ref path) = self.storage_path {
            validate_storage_path(path)?;
        }
        Ok(())
    }
}

fn default_deleted_task_lifespan() -> Option<u32> {
    Some(0)
}

fn default_storage_type() -> Option<String> {
    Some("json".to_string())
}

fn default_storage_path() -> Option<String> {
    let home = dirs::home_dir().expect("Could not determine home directory");
    Some(
        home.join(".config")
            .join("trtodo")
            .to_string_lossy()
            .to_string(),
    )
}

fn default_priority() -> Option<String> {
    Some("medium".to_string())
}

pub struct ConfigManager {
    storage: Box<dyn Storage>,
    old_storage_type: Option<String>,
}

#[allow(dead_code)]
impl ConfigManager {
    pub fn new(config_path: Option<&Path>) -> Result<Self, ConfigError> {
        let config_path = if let Some(path) = config_path {
            path.to_path_buf()
        } else if let Ok(path) = std::env::var("TRTODO_CONFIG") {
            PathBuf::from(path)
        } else {
            let home = dirs::home_dir().expect("Could not determine home directory");
            home.join(".config")
                .join("trtodo")
                .join("trtodo-config.json")
        };

        let storage =
            ConfigStorage::new(&config_path).map_err(|e| ConfigError::Storage(e.to_string()))?;
        let storage = Box::new(storage);
        let data = crate::models::StorageData {
            version: 1,
            tasks: Vec::new(),
            categories: Vec::new(),
            config: crate::config::Config::default(),
            current_category: None,
            last_sync: chrono::Utc::now(),
        };
        let config = data.config;

        config.validate()?;

        Ok(Self {
            storage,
            old_storage_type: None,
        })
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        let data = crate::models::StorageData {
            version: 1,
            tasks: Vec::new(),
            categories: Vec::new(),
            config: self.get_config().clone(),
            current_category: None,
            last_sync: chrono::Utc::now(),
        };
        self.storage
            .save(&data)
            .map_err(|e| ConfigError::Storage(e.to_string()))
    }

    #[allow(dead_code)]
    pub fn get(&self, key: &str) -> Option<String> {
        let config = self.get_config();
        match key {
            "deleted-task-lifespan" => config.deleted_task_lifespan.map(|v| v.to_string()),
            "storage.type" => config.storage_type.map(|v| v.to_string()),
            "storage.path" => config.storage_path.map(|v| v.to_string()),
            "default-category" => config.default_category.clone(),
            "default-priority" => config.default_priority.map(|v| v.to_string()),
            _ => None,
        }
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<(), ConfigError> {
        let mut data = self
            .storage
            .load()
            .map_err(|e| ConfigError::Storage(e.to_string()))?;
        let mut config = data.config.clone();

        match key {
            "deleted-task-lifespan" => {
                let value = validate_lifespan(value)?;
                config.deleted_task_lifespan = Some(value);
            }
            "storage.type" => {
                validate_storage_type(value)?;
                // Store old storage type for potential migration
                self.old_storage_type = Some(config.storage_type.clone().unwrap_or_default());
                config.storage_type = Some(value.to_string());
                eprintln!("Warning: Changing storage type may require data migration");
            }
            "storage.path" => {
                let path = validate_storage_path(value)?;
                config.storage_path = Some(path.to_string_lossy().to_string());
            }
            "default-category" => {
                // Note: Category validation would happen here once we have access to the storage layer
                config.default_category = Some(value.to_string());
            }
            "default-priority" => {
                validate_priority(value)?;
                config.default_priority = Some(value.to_string());
            }
            _ => {
                return Err(ConfigError::InvalidKey(key.to_string()));
            }
        }
        config.validate()?;
        data.config = config;
        self.storage
            .save(&data)
            .map_err(|e| ConfigError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn unset(&mut self, key: &str) -> Result<(), ConfigError> {
        let mut config = self.get_config();
        match key {
            "deleted-task-lifespan" => config.deleted_task_lifespan = None,
            "storage.type" => config.storage_type = None,
            "storage.path" => config.storage_path = None,
            "default-category" => config.default_category = None,
            "default-priority" => config.default_priority = None,
            _ => return Err(ConfigError::InvalidKey(key.to_string())),
        }
        let mut data = self.storage.load().unwrap();
        data.config = config;
        self.storage.save(&data)?;
        Ok(())
    }

    pub fn list(&self) -> Vec<(String, String, bool)> {
        let config = self.get_config();
        vec![
            (
                "deleted-task-lifespan".to_string(),
                config
                    .deleted_task_lifespan
                    .map_or_else(|| "0".to_string(), |v| v.to_string()),
                config.deleted_task_lifespan.is_none(),
            ),
            (
                "storage.type".to_string(),
                config
                    .storage_type
                    .clone()
                    .unwrap_or_else(|| "null".to_string()),
                config.storage_type.is_none(),
            ),
            (
                "storage.path".to_string(),
                config
                    .storage_path
                    .clone()
                    .unwrap_or_else(|| "null".to_string()),
                config.storage_path.is_none(),
            ),
            (
                "default-category".to_string(),
                config
                    .default_category
                    .clone()
                    .unwrap_or_else(|| "null".to_string()),
                config.default_category.is_none(),
            ),
            (
                "default-priority".to_string(),
                config
                    .default_priority
                    .clone()
                    .unwrap_or_else(|| "null".to_string()),
                config.default_priority.is_none(),
            ),
        ]
    }

    #[allow(dead_code)]
    pub fn needs_migration(&self) -> bool {
        self.old_storage_type.is_some()
            && self.old_storage_type.as_ref()
                != Some(
                    &self
                        .get_config()
                        .storage_type
                        .as_ref()
                        .cloned()
                        .unwrap_or_default(),
                )
    }

    #[allow(dead_code)]
    pub fn get_migration_info(&self) -> Option<(String, String)> {
        self.old_storage_type.as_ref().map(|old_type| {
            (
                old_type.clone(),
                self.get_config()
                    .storage_type
                    .as_ref()
                    .cloned()
                    .unwrap_or_default(),
            )
        })
    }

    pub fn get_storage(&self) -> Box<dyn Storage> {
        let config = self.get_config();
        let path = config.storage_path.as_ref().map_or_else(
            || {
                let home = dirs::home_dir().expect("Could not determine home directory");
                home.join(".config").join("trtodo").join("data.json")
            },
            |p| PathBuf::from(shellexpand::tilde(p).to_string()),
        );
        match config.storage_type.as_deref().unwrap_or("json") {
            "json" => Box::new(JsonStorage::new(path)),
            "sqlite" => Box::new(sqlite::SqliteStorage::new(&path).expect("Failed to create SQLite storage")),
            _ => Box::new(JsonStorage::new(path)), // Default to JSON storage
        }
    }

    fn get_config(&self) -> Config {
        self.storage.load().unwrap().config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{NamedTempFile, TempDir};

    #[test]
    fn test_config_manager() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");
        let mut manager = ConfigManager::new(Some(config_path.as_path())).unwrap();

        // Test setting storage type
        assert!(manager.set("storage.type", "json").is_ok());
        assert_eq!(manager.get("storage.type"), Some("json".to_string()));

        // Test setting storage path
        let storage_path = "~/.config/trtodo";
        assert!(manager.set("storage.path", storage_path).is_ok());
        assert_eq!(
            manager.get("storage.path"),
            Some(shellexpand::tilde(storage_path).to_string())
        );

        // Test setting default category
        assert!(manager.set("default-category", "work").is_ok());
        assert_eq!(manager.get("default-category"), Some("work".to_string()));

        // Test setting default priority
        assert!(manager.set("default-priority", "high").is_ok());
        assert_eq!(manager.get("default-priority"), Some("high".to_string()));

        // Test setting deleted task lifespan
        assert!(manager.set("deleted-task-lifespan", "7").is_ok());
        assert_eq!(manager.get("deleted-task-lifespan"), Some("7".to_string()));

        // Test unsetting values
        assert!(manager.unset("default-category").is_ok());
        assert_eq!(manager.get("default-category"), None);
    }

    #[test]
    fn test_config_manager_defaults() {
        let temp_file = NamedTempFile::new().unwrap();
        let manager = ConfigManager::new(Some(temp_file.path())).unwrap();

        assert_eq!(manager.get("deleted-task-lifespan"), None);
        assert_eq!(manager.get("storage.type"), None);
        assert_eq!(manager.get("default-category"), None);
        assert_eq!(manager.get("default-priority"), None);
    }

    #[test]
    fn test_config_manager_list() {
        let temp_file = NamedTempFile::new().unwrap();
        let manager = ConfigManager::new(Some(temp_file.path())).unwrap();

        let list = manager.list();
        assert_eq!(list.len(), 5);
        assert!(list.iter().all(|(_, _, is_default)| *is_default));
    }
}
