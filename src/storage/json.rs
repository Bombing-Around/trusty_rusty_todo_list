use super::{Storage, StorageError};
use crate::models::StorageData;
use std::path::{Path, PathBuf};

pub struct JsonStorage {
    path: PathBuf,
}

impl JsonStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Verifies that if a file exists at the target path and is non-empty, it
    /// contains valid JSON and is not in SQLite format. This guard prevents
    /// `save` from silently overwriting a SQLite database file (or any other
    /// non-JSON file) with JSON data, losing the original contents.
    ///
    /// Returns `Ok(())` if the file is absent, empty, or contains valid JSON.
    /// Returns `Err(StorageError::FormatMismatch)` if the file exists, is
    /// non-empty, and fails format validation (SQLite magic bytes or invalid
    /// JSON).
    ///
    /// This makes the guarantee local to the backend instead of depending on a
    /// filename convention maintained elsewhere. While normal configuration
    /// routes each backend to a distinct file (`trtodo-data.json` vs
    /// `trtodo-data.db`), a direct `JsonStorage::new(...)` call with an
    /// arbitrary path can reach any file. The check closes that gap.
    fn validate_target_format(&self) -> Result<(), StorageError> {
        if !self.path.exists() {
            return Ok(());
        }

        // Read the file to check its format
        let bytes = std::fs::read(&self.path)?;
        if bytes.is_empty() {
            return Ok(());
        }

        // SQLite database files start with this 16-byte magic header.
        const SQLITE_MAGIC: &[u8] = b"SQLite format 3\0";
        if bytes.len() >= SQLITE_MAGIC.len() && &bytes[..SQLITE_MAGIC.len()] == SQLITE_MAGIC {
            return Err(StorageError::FormatMismatch(
                "target file is a SQLite database, not JSON".to_string(),
            ));
        }

        // Attempt to parse as JSON to confirm it's valid. If this fails, the
        // file exists and is non-empty but is not valid JSON - reject the write.
        let contents = String::from_utf8_lossy(&bytes);
        serde_json::from_str::<serde_json::Value>(&contents).map_err(|_| {
            StorageError::FormatMismatch("target file exists and is not valid JSON".to_string())
        })?;

        Ok(())
    }
}

impl Storage for JsonStorage {
    fn save(&self, data: &StorageData) -> Result<(), StorageError> {
        // Validate data before saving
        data.validate()?;

        // Check that the target file (if it exists) is in JSON format, not
        // SQLite or some other format.
        self.validate_target_format()?;

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

    /// The `category use` context must survive a save/load cycle, since it is
    /// what makes the context persist between runs of the application.
    #[test]
    fn test_current_category_round_trip() {
        let dir = tempdir().unwrap();
        let json_path = dir.path().join("context.json");
        let storage = JsonStorage::new(&json_path);

        let mut data = create_test_data();
        assert_eq!(data.current_category, None);
        data.current_category = Some(7);
        storage.save(&data).unwrap();

        assert_eq!(storage.load().unwrap().current_category, Some(7));

        // ... and clearing it round-trips too.
        let mut data = storage.load().unwrap();
        data.current_category = None;
        storage.save(&data).unwrap();
        assert_eq!(storage.load().unwrap().current_category, None);
    }

    /// Data written before `current_category` existed has no such key; it must
    /// still load rather than failing deserialization.
    #[test]
    fn test_load_data_without_current_category_field() {
        let dir = tempdir().unwrap();
        let json_path = dir.path().join("legacy.json");

        let legacy = r#"{
            "version": 1,
            "tasks": [],
            "categories": [],
            "config": {},
            "last_sync": "2025-01-01T00:00:00Z"
        }"#;
        fs::write(&json_path, legacy).unwrap();

        let loaded = JsonStorage::new(&json_path).load().unwrap();
        assert_eq!(loaded.current_category, None);
    }

    /// Data written before `deleted_at` existed has no such key on its tasks;
    /// it must still load, coming back as "not deleted".
    #[test]
    fn test_load_task_without_deleted_at_field() {
        let dir = tempdir().unwrap();
        let json_path = dir.path().join("legacy_task.json");

        let legacy = r#"{
            "version": 1,
            "tasks": [
                {
                    "id": 1,
                    "title": "Old task",
                    "description": null,
                    "category_id": 0,
                    "completed": false,
                    "priority": "Medium",
                    "due_date": null,
                    "order": 0,
                    "created_at": "2025-01-01T00:00:00Z",
                    "updated_at": "2025-01-01T00:00:00Z"
                }
            ],
            "categories": [],
            "config": {},
            "last_sync": "2025-01-01T00:00:00Z"
        }"#;
        fs::write(&json_path, legacy).unwrap();

        let loaded = JsonStorage::new(&json_path).load().unwrap();
        assert_eq!(loaded.tasks.len(), 1);
        assert_eq!(loaded.tasks[0].deleted_at, None);
        assert!(!loaded.tasks[0].is_deleted());
    }

    /// Attempting to save over a file containing SQLite magic bytes must fail
    /// gracefully with a `FormatMismatch` error rather than silently overwriting
    /// the SQLite database and destroying the data.
    #[test]
    fn test_save_rejects_sqlite_magic_bytes() {
        let dir = tempdir().unwrap();
        let json_path = dir.path().join("sqlite.db");

        // Write SQLite magic bytes to the file
        let sqlite_magic = b"SQLite format 3\0\x04\x00\x00\x00";
        fs::write(&json_path, sqlite_magic).unwrap();
        let original_contents = fs::read(&json_path).unwrap();

        // Attempt to save JSON data over it; this must fail
        let storage = JsonStorage::new(&json_path);
        let test_data = create_test_data();
        let result = storage.save(&test_data);

        // Verify we got a format mismatch error
        assert!(result.is_err());
        match result {
            Err(StorageError::FormatMismatch(msg)) => {
                assert!(msg.contains("SQLite") || msg.contains("SQLite database"));
            }
            other => panic!("expected FormatMismatch error, got {:?}", other),
        }

        // Verify the file was not modified
        let current_contents = fs::read(&json_path).unwrap();
        assert_eq!(
            original_contents, current_contents,
            "file should not be modified when save rejects it"
        );
    }

    /// Saving to a nonexistent path must work normally, creating the file.
    #[test]
    fn test_save_to_nonexistent_path_succeeds() {
        let dir = tempdir().unwrap();
        let json_path = dir.path().join("nonexistent.json");

        assert!(!json_path.exists());

        let storage = JsonStorage::new(&json_path);
        let test_data = create_test_data();
        storage.save(&test_data).unwrap();

        assert!(json_path.exists());
        let loaded = storage.load().unwrap();
        assert_eq!(loaded.tasks.len(), 2);
        assert_eq!(loaded.categories.len(), 2);
    }

    /// Saving over an existing valid JSON file must work normally, replacing
    /// the old content with new data.
    #[test]
    fn test_save_over_existing_valid_json_succeeds() {
        let dir = tempdir().unwrap();
        let json_path = dir.path().join("existing.json");

        let storage = JsonStorage::new(&json_path);

        // Save initial data
        let initial_data = create_test_data();
        storage.save(&initial_data).unwrap();
        let loaded = storage.load().unwrap();
        assert_eq!(loaded.tasks.len(), 2);

        // Create new data with different content
        let mut new_data = StorageData::new();
        let mut category = Category::new("New".to_string(), None).unwrap();
        category.id = 1;
        new_data.categories.push(category.clone());
        let mut task = Task::new("New task".to_string(), 1, None, Priority::Low).unwrap();
        task.id = 1;
        new_data.tasks.push(task);

        // Save over the existing file - this must succeed
        storage.save(&new_data).unwrap();

        // Verify the new data was written
        let reloaded = storage.load().unwrap();
        assert_eq!(reloaded.tasks.len(), 1);
        assert_eq!(reloaded.categories.len(), 1);
        assert_eq!(reloaded.tasks[0].title, "New task");
    }

    /// Attempting to save over a file with invalid JSON (not SQLite, but also
    /// not valid JSON) must fail gracefully without modifying the file.
    #[test]
    fn test_save_rejects_invalid_json() {
        let dir = tempdir().unwrap();
        let json_path = dir.path().join("invalid.json");

        // Write some invalid JSON to the file
        let invalid_json = b"{ this is not valid json }";
        fs::write(&json_path, invalid_json).unwrap();
        let original_contents = fs::read(&json_path).unwrap();

        // Attempt to save JSON data over it; this must fail
        let storage = JsonStorage::new(&json_path);
        let test_data = create_test_data();
        let result = storage.save(&test_data);

        // Verify we got a format mismatch error
        assert!(result.is_err());
        match result {
            Err(StorageError::FormatMismatch(msg)) => {
                assert!(msg.contains("not valid JSON"));
            }
            other => panic!("expected FormatMismatch error, got {:?}", other),
        }

        // Verify the file was not modified
        let current_contents = fs::read(&json_path).unwrap();
        assert_eq!(
            original_contents, current_contents,
            "file should not be modified when save rejects it"
        );
    }
}
