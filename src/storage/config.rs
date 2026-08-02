use super::{Storage, StorageError};
use crate::config::Config;
use std::path::Path;

#[derive(Debug)]
pub struct ConfigStorage {
    path: std::path::PathBuf,
}

impl ConfigStorage {
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
    use tempfile::TempDir;

    #[test]
    fn test_config_storage() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");
        let storage = ConfigStorage::new(config_path).unwrap();

        let data = crate::models::StorageData {
            version: 1,
            tasks: vec![],
            categories: vec![],
            config: Config {
                deleted_task_lifespan: Some(7),
                storage_type: Some("json".to_string()),
                storage_path: Some("~/.config/trtodo".to_string()),
                default_category: Some("work".to_string()),
                default_priority: Some("medium".to_string()),
            },
            current_category: None,
            last_sync: chrono::Utc::now(),
        };

        // Test save
        storage.save(&data).unwrap();

        // Test load
        let loaded_data = storage.load().unwrap();
        assert_eq!(loaded_data.config.deleted_task_lifespan, Some(7));
        assert_eq!(loaded_data.config.storage_type, Some("json".to_string()));
        assert_eq!(
            loaded_data.config.storage_path,
            Some("~/.config/trtodo".to_string())
        );
        assert_eq!(
            loaded_data.config.default_category,
            Some("work".to_string())
        );
        assert_eq!(
            loaded_data.config.default_priority,
            Some("medium".to_string())
        );
    }

    #[test]
    fn test_empty_config_storage() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");
        let storage = ConfigStorage::new(config_path).unwrap();

        let loaded_data = storage.load().unwrap();
        assert_eq!(loaded_data.config.deleted_task_lifespan, None);
        assert_eq!(loaded_data.config.storage_type, None);
        assert_eq!(loaded_data.config.storage_path, None);
        assert_eq!(loaded_data.config.default_category, None);
        assert_eq!(loaded_data.config.default_priority, None);
    }
}
