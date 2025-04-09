use super::{Storage, StorageError};
use crate::models::StorageData;
use crate::config::Config;
use std::path::PathBuf;
use chrono::Utc;
use shellexpand;

pub struct JsonStorage {
    path: PathBuf,
}

impl JsonStorage {
    pub fn new(config: Config) -> Result<Self, StorageError> {
        let path = config.storage_path
            .ok_or_else(|| StorageError::Storage("Storage path not configured".to_string()))?;
        let path = PathBuf::from(shellexpand::tilde(&path).to_string());
        Ok(Self {
            path,
        })
    }
}

impl Storage for JsonStorage {
    fn save(&self, data: &StorageData) -> Result<(), StorageError> {
        // Validate data before saving
        data.validate()?;

        // Create parent directories if they don't exist
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(data)?;
        std::fs::write(&self.path, json)?;

        // Verify the write was successful by reading back
        let contents = std::fs::read_to_string(&self.path)?;
        let read_data: StorageData = serde_json::from_str(&contents)?;

        // Verify data integrity
        if read_data.tasks.len() != data.tasks.len()
            || read_data.categories.len() != data.categories.len()
        {
            return Err(StorageError::Storage(
                "Data integrity check failed".to_string(),
            ));
        }

        Ok(())
    }

    fn load(&self) -> Result<StorageData, StorageError> {
        if !self.path.exists() {
            return Ok(StorageData {
                version: 1,
                tasks: Vec::new(),
                categories: Vec::new(),
                config: Config::with_defaults(),
                current_category: None,
                last_sync: Utc::now(),
            });
        }

        let contents = std::fs::read_to_string(&self.path)?;
        if contents.trim().is_empty() {
            return Ok(StorageData::new());
        }

        let data: StorageData = serde_json::from_str(&contents)?;
        data.validate()?;
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_json_storage() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage_path = temp_dir.path().join("tasks.json");
        let mut config = Config::default();
        config.storage_path = Some(storage_path.to_str().unwrap().to_string());
        let storage = JsonStorage::new(config);
        assert!(storage.is_ok());
    }

    #[test]
    fn test_json_storage_custom_path() {
        let temp_dir = tempfile::Builder::new()
            .prefix("trtodo_test_json")
            .tempdir()
            .expect("Failed to create temporary directory");
        let storage_path = temp_dir.path().join("test_custom.json");
        
        let mut config = Config::default();
        config.storage_path = Some(storage_path.to_str().unwrap().to_string());
        
        let storage = JsonStorage::new(config).expect("Failed to create storage");
        assert!(storage.load().is_ok());
    }
}
