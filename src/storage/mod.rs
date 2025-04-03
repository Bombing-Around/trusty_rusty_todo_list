use std::path::Path;
use thiserror::Error;
use crate::models::{StorageData, Task, Category, Priority};

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Storage error: {0}")]
    Storage(String),
}

#[derive(Debug, Clone, Copy)]
pub enum StorageType {
    Json,
    Sqlite,
}

pub trait Storage: Send + Sync {
    fn save(&self, data: &StorageData) -> Result<(), StorageError>;
    fn load(&self) -> Result<StorageData, StorageError>;
}

pub struct JsonStorage {
    path: std::path::PathBuf,
}

impl JsonStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
}

impl Storage for JsonStorage {
    fn save(&self, data: &StorageData) -> Result<(), StorageError> {
        let json = serde_json::to_string_pretty(data)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }

    fn load(&self) -> Result<StorageData, StorageError> {
        if !self.path.exists() {
            return Ok(StorageData {
                tasks: Vec::new(),
                categories: Vec::new(),
            });
        }

        let contents = std::fs::read_to_string(&self.path)?;
        let data: StorageData = serde_json::from_str(&contents)?;
        Ok(data)
    }
}

pub fn create_storage(
    storage_type: StorageType,
    path: &Path,
) -> Result<Box<dyn Storage>, StorageError> {
    match storage_type {
        StorageType::Json => {
            let storage = JsonStorage::new(path);
            Ok(Box::new(storage))
        }
        StorageType::Sqlite => {
            // TODO: Implement SQLite storage
            Err(StorageError::Storage(
                "SQLite storage not yet implemented".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_json_storage() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = JsonStorage::new(temp_file.path());

        // Test data
        let test_data = StorageData {
            tasks: vec![Task {
                id: 1,
                title: "Test Task".to_string(),
                category_id: 1,
                completed: false,
                priority: Priority::Medium,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            }],
            categories: vec![Category {
                id: 1,
                name: "Test Category".to_string(),
                created_at: chrono::Utc::now(),
            }],
        };

        // Test save
        storage.save(&test_data).unwrap();

        // Test load
        let loaded_data = storage.load().unwrap();
        assert_eq!(loaded_data.tasks.len(), 1);
        assert_eq!(loaded_data.categories.len(), 1);
        assert_eq!(loaded_data.tasks[0].title, "Test Task");
        assert_eq!(loaded_data.categories[0].name, "Test Category");
    }

    #[test]
    fn test_storage_factory() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = create_storage(StorageType::Json, temp_file.path()).unwrap();

        // Test that we can use the storage
        let test_data = StorageData {
            tasks: vec![],
            categories: vec![],
        };

        storage.save(&test_data).unwrap();
        let loaded_data = storage.load().unwrap();
        assert_eq!(loaded_data.tasks.len(), 0);
        assert_eq!(loaded_data.categories.len(), 0);
    }
}
