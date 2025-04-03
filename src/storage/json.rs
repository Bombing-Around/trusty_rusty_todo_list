use super::Storage;
use super::StorageError;
use crate::models::StorageData;
use std::path::Path;

#[allow(dead_code)]
pub struct JsonStorage {
    path: std::path::PathBuf,
}

#[allow(dead_code)]
impl JsonStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }
}

impl Storage for JsonStorage {
    fn save(&self, data: &StorageData) -> Result<(), StorageError> {
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
                tasks: Vec::new(),
                categories: Vec::new(),
                config: crate::config::Config::default(),
            });
        }

        let contents = std::fs::read_to_string(&self.path)?;

        // If the file is empty, return empty storage data
        if contents.trim().is_empty() {
            return Ok(StorageData {
                tasks: Vec::new(),
                categories: Vec::new(),
                config: crate::config::Config::default(),
            });
        }

        let data: StorageData = serde_json::from_str(&contents)?;
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Category, Priority, Task};
    use chrono::Utc;
    use tempfile::NamedTempFile;

    // Helper function to create test data
    fn create_test_data() -> StorageData {
        let now = Utc::now();
        StorageData {
            tasks: vec![
                Task {
                    id: 1,
                    title: "High Priority Task".to_string(),
                    category_id: 1,
                    completed: false,
                    priority: Priority::High,
                    created_at: now,
                    updated_at: now,
                },
                Task {
                    id: 2,
                    title: "Medium Priority Task".to_string(),
                    category_id: 1,
                    completed: true,
                    priority: Priority::Medium,
                    created_at: now,
                    updated_at: now,
                },
                Task {
                    id: 3,
                    title: "Low Priority Task".to_string(),
                    category_id: 2,
                    completed: false,
                    priority: Priority::Low,
                    created_at: now,
                    updated_at: now,
                },
                Task {
                    id: 4,
                    title: "Deleted Task".to_string(),
                    category_id: 0, // Deleted category
                    completed: false,
                    priority: Priority::Medium,
                    created_at: now,
                    updated_at: now,
                },
            ],
            categories: vec![
                Category {
                    id: 1,
                    name: "Work".to_string(),
                    created_at: now,
                },
                Category {
                    id: 2,
                    name: "Home".to_string(),
                    created_at: now,
                },
            ],
            config: crate::config::Config::default(),
        }
    }

    #[test]
    fn test_json_storage() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = JsonStorage::new(temp_file.path());

        // Test data
        let test_data = create_test_data();

        // Test save
        storage.save(&test_data).unwrap();

        // Test load
        let loaded_data = storage.load().unwrap();
        assert_eq!(loaded_data.tasks.len(), 4);
        assert_eq!(loaded_data.categories.len(), 2);
        assert_eq!(loaded_data.tasks[0].title, "High Priority Task");
        assert_eq!(loaded_data.categories[0].name, "Work");
        assert_eq!(loaded_data.tasks[0].priority, Priority::High);
    }

    #[test]
    fn test_empty_json_storage() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = JsonStorage::new(temp_file.path());

        let loaded_data = storage.load().unwrap();
        assert_eq!(loaded_data.tasks.len(), 0);
        assert_eq!(loaded_data.categories.len(), 0);
    }

    #[test]
    fn test_data_integrity() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = JsonStorage::new(temp_file.path());
        let test_data = create_test_data();

        // Test save and verify integrity
        storage.save(&test_data).unwrap();
        let loaded_data = storage.load().unwrap();

        // Verify all data matches
        assert_eq!(loaded_data.tasks.len(), test_data.tasks.len());
        assert_eq!(loaded_data.categories.len(), test_data.categories.len());

        // Verify task details
        for (loaded, original) in loaded_data.tasks.iter().zip(test_data.tasks.iter()) {
            assert_eq!(loaded.id, original.id);
            assert_eq!(loaded.title, original.title);
            assert_eq!(loaded.category_id, original.category_id);
            assert_eq!(loaded.completed, original.completed);
            assert_eq!(loaded.priority, original.priority);
        }

        // Verify category details
        for (loaded, original) in loaded_data
            .categories
            .iter()
            .zip(test_data.categories.iter())
        {
            assert_eq!(loaded.id, original.id);
            assert_eq!(loaded.name, original.name);
        }
    }
}
