use crate::config::Config;
use crate::config::ConfigManager;
use crate::models::StorageData;
use crate::storage::json::JsonStorage;
use crate::storage::Storage;
use std::path::PathBuf;
use tempfile::TempDir;

#[allow(dead_code)] // These fields and methods are used in tests
pub struct TestStorage {
    temp_dir: TempDir,
    storage: Box<dyn Storage>,
}

#[allow(dead_code)] // These methods are used in tests
impl TestStorage {
    pub fn new() -> Self {
        let temp_dir = tempfile::Builder::new()
            .prefix("trtodo_test")
            .tempdir()
            .expect("Failed to create temporary directory");

        let storage_path = temp_dir.path().join("test_storage.json");
        let mut config = Config::default();
        config.storage_path = Some(storage_path.to_str().unwrap().to_string());

        let storage = Box::new(JsonStorage::new(config).expect("Failed to create test storage"));

        // Initialize with empty data
        let data = StorageData::new();
        storage
            .save(&data)
            .expect("Failed to initialize test storage");

        Self { temp_dir, storage }
    }

    pub fn storage(&self) -> &dyn Storage {
        &*self.storage
    }

    pub fn storage_mut(&mut self) -> &mut dyn Storage {
        &mut *self.storage
    }

    pub fn path(&self) -> PathBuf {
        self.temp_dir.path().to_path_buf()
    }
}

impl Drop for TestStorage {
    fn drop(&mut self) {
        // TempDir will automatically clean up on drop
    }
}

/// Creates a test configuration manager that uses a temporary directory for both config and data storage.
/// This ensures that tests don't interfere with the user's actual configuration and data.
#[allow(dead_code)]
pub fn create_test_config_manager() -> (ConfigManager, TempDir) {
    let temp_dir = tempfile::Builder::new()
        .prefix("trtodo_test")
        .tempdir()
        .expect("Failed to create temporary directory");

    let mut config = Config::default();
    let storage_path = temp_dir
        .path()
        .join("test-data.json")
        .to_str()
        .unwrap()
        .to_string();
    config.storage_path = Some(storage_path.clone());
    config.storage_type = Some("json".to_string());
    config.default_priority = Some("medium".to_string());

    let storage = Box::new(JsonStorage::new(config).expect("Failed to create test storage"));

    // Initialize with empty data
    let data = StorageData::new();
    storage
        .save(&data)
        .expect("Failed to initialize test storage");

    let mut config_manager = ConfigManager::with_storage(storage);
    config_manager
        .set("storage.path", &storage_path)
        .expect("Failed to set storage.path");

    (config_manager, temp_dir)
}

#[allow(dead_code)]
pub fn create_test_storage() -> (Box<dyn Storage>, tempfile::TempDir) {
    let temp_dir = tempfile::Builder::new()
        .prefix("trtodo_test")
        .tempdir()
        .expect("Failed to create temporary directory");
    let storage_path = temp_dir.path().join("test.json");

    let mut config = Config::default();
    config.storage_path = Some(storage_path.to_str().unwrap().to_string());

    let storage = Box::new(JsonStorage::new(config).expect("Failed to create test storage"));

    // Initialize with empty data
    let data = StorageData::new();
    storage
        .save(&data)
        .expect("Failed to initialize test storage");

    (storage, temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Category, Priority, Task};

    #[test]
    fn test_storage_initialization() {
        let test_storage = TestStorage::new();
        let data = test_storage
            .storage()
            .load()
            .expect("Failed to load storage");
        assert!(data.tasks.is_empty());
        assert!(data.categories.is_empty());
    }

    #[test]
    fn test_category_operations() {
        let mut test_storage = TestStorage::new();
        let storage = test_storage.storage_mut();

        // Add category
        let category = Category::new("Test".to_string(), None).expect("Failed to create category");
        storage
            .add_category(category.clone())
            .expect("Failed to add category");

        // Verify category was added
        let data = storage.load().expect("Failed to load storage");
        assert_eq!(data.categories.len(), 1);
        assert_eq!(data.categories[0].name, "Test");
    }

    #[test]
    fn test_task_operations() {
        let mut test_storage = TestStorage::new();
        let storage = test_storage.storage_mut();

        // Add category first
        let category = Category::new("Test".to_string(), None).expect("Failed to create category");
        storage
            .add_category(category.clone())
            .expect("Failed to add category");

        // Add task
        let task = Task::new("Test Task".to_string(), category.id, None, Priority::Medium)
            .expect("Failed to create task");

        storage.add_task(task.clone()).expect("Failed to add task");

        // Verify task was added
        let data = storage.load().expect("Failed to load storage");
        assert_eq!(data.tasks.len(), 1);
        assert_eq!(data.tasks[0].title, "Test Task");
    }

    #[test]
    fn test_config_manager() {
        let (config_manager, _temp_dir) = create_test_config_manager();

        // Verify storage path is set to our temporary file
        let storage_path = config_manager
            .get("storage.path")
            .expect("Storage path not set");
        assert!(storage_path.contains("trtodo_test"));

        // Verify we can get the storage
        let storage = config_manager.get_storage();
        assert!(storage.load().is_ok());
    }
}
