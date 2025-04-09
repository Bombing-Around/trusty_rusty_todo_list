use super::StorageError;
use crate::config::Config;
use crate::storage::{Storage, StorageType};
use std::path::Path;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StorageConfig {
    pub storage_type: StorageType,
    pub storage_path: Option<std::path::PathBuf>,
}

impl StorageConfig {
    #[allow(dead_code)]
    pub fn from_config_manager(manager: &crate::config::ConfigManager) -> Self {
        let storage_type = manager
            .get("storage.type")
            .and_then(|s| match s.as_str() {
                "sqlite" => Some(StorageType::Sqlite),
                "json" => Some(StorageType::Json),
                _ => None,
            })
            .unwrap_or(StorageType::Json);

        let storage_path = manager
            .get("storage.path")
            .map(|s| std::path::PathBuf::from(shellexpand::tilde(&s).to_string()));

        Self {
            storage_type,
            storage_path,
        }
    }
}

#[derive(Debug)]
pub struct ConfigStorage {
    path: std::path::PathBuf,
}

impl ConfigStorage {
    #[allow(dead_code)]
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        Ok(Self {
            path: path.as_ref().to_path_buf(),
        })
    }
}

impl Storage for ConfigStorage {
    fn save(&self, data: &crate::models::StorageData) -> Result<(), StorageError> {
        // Convert StorageData to Config
        let config = Config {
            deleted_task_lifespan: data.config.deleted_task_lifespan,
            storage_type: data.config.storage_type.clone(),
            storage_path: data.config.storage_path.clone(),
            default_category: data.config.default_category.clone(),
            default_priority: data.config.default_priority.clone(),
        };

        // Create parent directories if they don't exist
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(&config)?;
        std::fs::write(&self.path, json)?;

        // Verify the write was successful by reading back
        let contents = std::fs::read_to_string(&self.path)?;
        let _: Config = serde_json::from_str(&contents)?;

        Ok(())
    }

    fn load(&self) -> Result<crate::models::StorageData, StorageError> {
        if !self.path.exists() {
            return Ok(crate::models::StorageData {
                version: 1,
                tasks: Vec::new(),
                categories: Vec::new(),
                config: Config::default(),
                current_category: None,
                last_sync: chrono::Utc::now(),
            });
        }

        let contents = std::fs::read_to_string(&self.path)?;

        // If the file is empty, return default config
        if contents.trim().is_empty() {
            return Ok(crate::models::StorageData {
                version: 1,
                tasks: Vec::new(),
                categories: Vec::new(),
                config: Config::default(),
                current_category: None,
                last_sync: chrono::Utc::now(),
            });
        }

        let config: Config = serde_json::from_str(&contents)?;
        Ok(crate::models::StorageData {
            version: 1,
            tasks: Vec::new(),
            categories: Vec::new(),
            config,
            current_category: None,
            last_sync: chrono::Utc::now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_utils::create_test_config_manager;

    #[test]
    fn test_storage_config_from_manager() {
        let (manager, temp_dir) = create_test_config_manager();
        let config = StorageConfig::from_config_manager(&manager);
        assert_eq!(config.storage_type, StorageType::Json);
        assert_eq!(config.storage_path, Some(temp_dir.path().join("test-data.json")));
    }

    #[test]
    fn test_storage_config_from_manager_with_custom_type() {
        let (mut manager, temp_dir) = create_test_config_manager();
        manager.set("storage.type", "sqlite").unwrap();
        let config = StorageConfig::from_config_manager(&manager);
        assert_eq!(config.storage_type, StorageType::Sqlite);
        assert_eq!(config.storage_path, Some(temp_dir.path().join("test-data.json")));
    }

    #[test]
    fn test_storage_config_from_manager_with_invalid_type() {
        let (manager, temp_dir) = create_test_config_manager();

        let mut data = manager.get_storage_ref().load().unwrap();
        data.config.storage_type = Some("invalid".to_string());
        manager.get_storage_ref().save(&data).unwrap();
        
        let config = StorageConfig::from_config_manager(&manager);
        assert_eq!(config.storage_type, StorageType::Json); // Should default to Json
        assert_eq!(config.storage_path, Some(temp_dir.path().join("test-data.json")));
    }
}
