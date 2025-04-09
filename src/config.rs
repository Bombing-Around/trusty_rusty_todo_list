use crate::models::StorageError;
use crate::storage::{json::JsonStorage, sqlite, Storage};
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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_deleted_task_lifespan")]
    pub deleted_task_lifespan: Option<u32>,
    #[serde(default)]
    pub storage_type: Option<String>,
    #[serde(default)]
    pub storage_path: Option<String>,
    #[serde(default)]
    pub default_category: Option<String>,
    #[serde(default = "default_priority")]
    pub default_priority: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            deleted_task_lifespan: None,
            storage_type: None,
            storage_path: None,
            default_category: None,
            default_priority: default_priority(),
        }
    }
}

impl Config {
    pub fn with_defaults() -> Self {
        Self {
            deleted_task_lifespan: default_deleted_task_lifespan(),
            storage_type: default_storage_type(),
            storage_path: default_storage_path(),
            default_category: None,
            default_priority: default_priority(),
        }
    }

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
            .join("data.json")
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

impl ConfigManager {
    pub fn new(config_path: Option<&Path>) -> Result<Self, StorageError> {
        let path = config_path.map_or_else(
            || {
                let home = dirs::home_dir().expect("Could not determine home directory");
                home.join(".config").join("trtodo").join("config.json")
            },
            |p| p.to_path_buf(),
        );

        let config = Config {
            storage_path: Some(path.to_str().unwrap().to_string()),
            ..Default::default()
        };

        let storage = Box::new(JsonStorage::new(config)?);
        let mut config_manager = Self {
            storage,
            old_storage_type: None,
        };

        // Initialize with default values if this is a new config
        if !path.exists() {
            config_manager
                .set("deleted-task-lifespan", "0")
                .map_err(|e| StorageError::Storage(e.to_string()))?;
            config_manager
                .set("storage.type", "json")
                .map_err(|e| StorageError::Storage(e.to_string()))?;
            config_manager
                .set("default-priority", "medium")
                .map_err(|e| StorageError::Storage(e.to_string()))?;
        }

        Ok(config_manager)
    }

    #[allow(dead_code)]
    pub fn with_storage(storage: Box<dyn Storage>) -> Self {
        let config_manager = Self {
            storage,
            old_storage_type: None,
        };

        // Initialize with default values
        let mut data = config_manager.storage.load().unwrap();
        data.config = Config::with_defaults();
        config_manager
            .storage
            .save(&data)
            .expect("Failed to initialize storage");

        config_manager
    }

    #[allow(dead_code)]
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
        let mut list = Vec::new();
        let defaults = Config::with_defaults();

        // Add storage type
        list.push((
            "storage.type".to_string(),
            defaults.storage_type.unwrap_or_else(|| "null".to_string()),
            true,
        ));

        // Add storage path
        list.push((
            "storage.path".to_string(),
            defaults.storage_path.unwrap_or_else(|| "null".to_string()),
            true,
        ));

        // Add deleted task lifespan
        list.push((
            "deleted-task-lifespan".to_string(),
            defaults
                .deleted_task_lifespan
                .map(|v| v.to_string())
                .unwrap_or_else(|| "null".to_string()),
            true,
        ));

        // Add default priority
        list.push((
            "default-priority".to_string(),
            defaults
                .default_priority
                .unwrap_or_else(|| "null".to_string()),
            true,
        ));

        // Add any custom values from the config file
        if let Ok(data) = self.storage.load() {
            let config = data.config;
            if let Some(value) = config.deleted_task_lifespan {
                list.push((
                    "deleted-task-lifespan".to_string(),
                    value.to_string(),
                    false,
                ));
            }
            if let Some(value) = config.storage_type {
                list.push(("storage.type".to_string(), value, false));
            }
            if let Some(value) = config.storage_path {
                list.push(("storage.path".to_string(), value, false));
            }
            if let Some(value) = config.default_category {
                list.push(("default-category".to_string(), value, false));
            }
            if let Some(value) = config.default_priority {
                list.push(("default-priority".to_string(), value, false));
            }
        }

        list
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

    pub fn create_storage(&self) -> Result<Box<dyn Storage>, StorageError> {
        let path = self.get("storage.path").ok_or_else(|| {
            StorageError::Storage("Storage path not configured".to_string())
        })?;

        let config = Config {
            storage_path: Some(path),
            storage_type: self.get("storage.type"),
            ..Default::default()
        };

        match config.storage_type.as_deref() {
            Some("json") => {
                let storage = JsonStorage::new(config)?;
                Ok(Box::new(storage))
            }
            Some("sqlite") => {
                let storage = sqlite::SqliteStorage::new(config)?;
                Ok(Box::new(storage))
            }
            _ => {
                let storage = JsonStorage::new(config)?;
                Ok(Box::new(storage))
            }
        }
    }

    fn get_config(&self) -> Config {
        self.storage.load().unwrap().config
    }

    pub fn get_storage(&self) -> Box<dyn Storage> {
        self.create_storage().expect("Failed to create storage")
    }

    #[allow(dead_code)]
    pub fn get_storage_ref(&self) -> &dyn Storage {
        &*self.storage
    }
}

#[cfg(test)]
mod tests {
    use crate::storage::test_utils::create_test_config_manager;

    #[test]
    fn test_config_manager() {
        let (mut manager, _temp_dir) = create_test_config_manager();

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
        let (manager, _temp_dir) = create_test_config_manager();

        // Check that the defaults are set correctly
        assert_eq!(manager.get("deleted-task-lifespan"), Some("0".to_string()));
        assert_eq!(manager.get("storage.type"), Some("json".to_string()));
        assert_eq!(manager.get("default-category"), None);
        assert_eq!(manager.get("default-priority"), Some("medium".to_string()));
    }

    #[test]
    fn test_config_manager_list() {
        let (manager, _temp_dir) = create_test_config_manager();
        let list = manager.list();
        assert!(!list.is_empty());

        // Check that default values are present
        let has_storage_type = list.iter().any(|(key, value, is_default)| {
            key == "storage.type" && (value == "json" || value == "null") && *is_default
        });
        assert!(
            has_storage_type,
            "storage.type should be present with default value"
        );

        let has_storage_path = list.iter().any(|(key, value, is_default)| {
            key == "storage.path" && (value.contains("data.json") || value == "null") && *is_default
        });
        assert!(
            has_storage_path,
            "storage.path should be present with default value"
        );

        let has_deleted_task_lifespan = list.iter().any(|(key, value, is_default)| {
            key == "deleted-task-lifespan" && (value == "0" || value == "null") && *is_default
        });
        assert!(
            has_deleted_task_lifespan,
            "deleted-task-lifespan should be present with default value"
        );

        let has_default_priority = list.iter().any(|(key, value, is_default)| {
            key == "default-priority" && (value == "medium" || value == "null") && *is_default
        });
        assert!(
            has_default_priority,
            "default-priority should be present with default value"
        );
    }
}
