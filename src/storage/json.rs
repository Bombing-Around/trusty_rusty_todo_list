use crate::models::StorageData;
use std::path::{Path, PathBuf};
use super::{Storage, StorageError};

pub struct JsonStorage {
    path: PathBuf,
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
            return Ok(StorageData::new());
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
    use crate::models::{Category, Priority, Task};
    use std::fs;
    use tempfile::tempdir;

    fn create_test_data() -> StorageData {
        let mut data = StorageData::new();

        // Create test categories
        let work =
            Category::new("Work".to_string(), Some("Work related tasks".to_string())).unwrap();
        let personal =
            Category::new("Personal".to_string(), Some("Personal tasks".to_string())).unwrap();

        data.categories.push(work.clone());
        data.categories.push(personal.clone());

        // Create test tasks
        let task1 = Task::new(
            "Complete project".to_string(),
            work.id,
            Some("Finish the todo list project".to_string()),
            Priority::High,
        )
        .unwrap();

        let task2 = Task::new(
            "Buy groceries".to_string(),
            personal.id,
            Some("Get milk and bread".to_string()),
            Priority::Medium,
        )
        .unwrap();

        data.tasks.push(task1);
        data.tasks.push(task2);

        data
    }

    #[test]
    fn test_json_storage() {
        let dir = tempdir().unwrap();
        let json_path = dir.path().join("test.json");
        let storage = JsonStorage::new(&json_path);

        let test_data = create_test_data();
        storage.save(&test_data).unwrap();

        let loaded_data = storage.load().unwrap();
        assert_eq!(loaded_data.tasks.len(), 2);
        assert_eq!(loaded_data.categories.len(), 2);

        // Verify task data
        let task = loaded_data.tasks.first().unwrap();
        assert_eq!(task.title, "Complete project");
        assert_eq!(task.priority, Priority::High);
        assert!(task.description.is_some());
        assert_eq!(
            task.description.as_ref().unwrap(),
            "Finish the todo list project"
        );

        // Verify category data
        let category = loaded_data.categories.first().unwrap();
        assert_eq!(category.name, "Work");
        assert!(category.description.is_some());
        assert_eq!(category.description.as_ref().unwrap(), "Work related tasks");
    }

    #[test]
    fn test_empty_json_storage() {
        let dir = tempdir().unwrap();
        let json_path = dir.path().join("empty.json");
        let storage = JsonStorage::new(&json_path);

        let loaded_data = storage.load().unwrap();
        assert_eq!(loaded_data.tasks.len(), 0);
        assert_eq!(loaded_data.categories.len(), 0);
    }

    #[test]
    fn test_data_integrity() {
        let dir = tempdir().unwrap();
        let json_path = dir.path().join("integrity.json");
        let storage = JsonStorage::new(&json_path);

        let test_data = create_test_data();
        storage.save(&test_data).unwrap();

        // Verify file exists and has content
        assert!(json_path.exists());
        let contents = fs::read_to_string(&json_path).unwrap();
        assert!(!contents.is_empty());

        // Verify JSON structure
        let json: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert!(json.is_object());
        assert!(json.get("version").is_some());
        assert!(json.get("tasks").is_some());
        assert!(json.get("categories").is_some());
        assert!(json.get("last_sync").is_some());

        // Verify tasks array
        let tasks = json.get("tasks").unwrap().as_array().unwrap();
        assert_eq!(tasks.len(), 2);
        let first_task = &tasks[0];
        assert_eq!(
            first_task.get("title").unwrap().as_str().unwrap(),
            "Complete project"
        );
        assert_eq!(
            first_task.get("priority").unwrap().as_str().unwrap(),
            "High"
        );
        assert!(first_task.get("description").is_some());

        // Verify categories array
        let categories = json.get("categories").unwrap().as_array().unwrap();
        assert_eq!(categories.len(), 2);
        let first_category = &categories[0];
        assert_eq!(
            first_category.get("name").unwrap().as_str().unwrap(),
            "Work"
        );
        assert!(first_category.get("description").is_some());
    }
}
