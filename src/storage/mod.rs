use crate::models::StorageData;
#[allow(unused_imports)]
use crate::models::{Category, Priority, Task};
use std::path::Path;
use thiserror::Error;

#[cfg(test)]
mod test_utils {
    pub use super::*;
    pub use tempfile::NamedTempFile;
}

mod migrations;
mod sqlite;
pub(crate) use sqlite::SqliteStorage;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Storage error: {0}")]
    Storage(String),
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum StorageType {
    Json,
    Sqlite,
}

#[allow(dead_code)]
pub trait Storage: Send + Sync {
    fn save(&self, data: &StorageData) -> Result<(), StorageError>;
    fn load(&self) -> Result<StorageData, StorageError>;

    // Convenience methods for common operations
    fn add_task(&self, task: Task) -> Result<(), StorageError> {
        let mut data = self.load()?;
        data.tasks.push(task);
        self.save(&data)
    }

    fn delete_task(&self, task_id: u64) -> Result<(), StorageError> {
        let mut data = self.load()?;
        data.tasks.retain(|t| t.id != task_id);
        self.save(&data)
    }

    fn update_task(&self, task: Task) -> Result<(), StorageError> {
        let mut data = self.load()?;
        if let Some(existing_task) = data.tasks.iter_mut().find(|t| t.id == task.id) {
            *existing_task = task;
            self.save(&data)
        } else {
            Err(StorageError::Storage(format!(
                "Task with id {} not found",
                task.id
            )))
        }
    }

    fn get_task(&self, task_id: u64) -> Result<Option<Task>, StorageError> {
        let data = self.load()?;
        Ok(data.tasks.into_iter().find(|t| t.id == task_id))
    }

    fn add_category(&self, category: Category) -> Result<(), StorageError> {
        let mut data = self.load()?;
        data.categories.push(category);
        self.save(&data)
    }

    fn delete_category(&self, category_id: u64) -> Result<(), StorageError> {
        let mut data = self.load()?;
        // Check if category has any tasks
        if data.tasks.iter().any(|t| t.category_id == category_id) {
            return Err(StorageError::Storage(format!(
                "Cannot delete category {}: it has associated tasks",
                category_id
            )));
        }
        data.categories.retain(|c| c.id != category_id);
        self.save(&data)
    }

    fn update_category(&self, category: Category) -> Result<(), StorageError> {
        let mut data = self.load()?;
        if let Some(existing_category) = data.categories.iter_mut().find(|c| c.id == category.id) {
            *existing_category = category;
            self.save(&data)
        } else {
            Err(StorageError::Storage(format!(
                "Category with id {} not found",
                category.id
            )))
        }
    }

    fn get_category(&self, category_id: u64) -> Result<Option<Category>, StorageError> {
        let data = self.load()?;
        Ok(data.categories.into_iter().find(|c| c.id == category_id))
    }

    fn get_tasks_by_category(&self, category_id: u64) -> Result<Vec<Task>, StorageError> {
        let data = self.load()?;
        Ok(data
            .tasks
            .into_iter()
            .filter(|t| t.category_id == category_id)
            .collect())
    }

    fn get_tasks_by_priority(&self, priority: Priority) -> Result<Vec<Task>, StorageError> {
        let data = self.load()?;
        Ok(data
            .tasks
            .into_iter()
            .filter(|t| t.priority == priority)
            .collect())
    }

    fn get_completed_tasks(&self) -> Result<Vec<Task>, StorageError> {
        let data = self.load()?;
        Ok(data.tasks.into_iter().filter(|t| t.completed).collect())
    }

    fn get_incomplete_tasks(&self) -> Result<Vec<Task>, StorageError> {
        let data = self.load()?;
        Ok(data.tasks.into_iter().filter(|t| !t.completed).collect())
    }

    fn search_tasks(&self, query: &str) -> Result<Vec<Task>, StorageError> {
        let data = self.load()?;
        let query = query.to_lowercase();
        Ok(data
            .tasks
            .into_iter()
            .filter(|t| t.title.to_lowercase().contains(&query))
            .collect())
    }

    fn get_next_task_id(&self) -> Result<u64, StorageError> {
        let data = self.load()?;
        Ok(data.tasks.iter().map(|t| t.id).max().unwrap_or(0) + 1)
    }

    fn get_next_category_id(&self) -> Result<u64, StorageError> {
        let data = self.load()?;
        Ok(data.categories.iter().map(|c| c.id).max().unwrap_or(0) + 1)
    }

    // Additional convenience methods for README behaviors
    fn get_tasks_by_title(&self, title: &str) -> Result<Vec<Task>, StorageError> {
        let data = self.load()?;
        let title = title.to_lowercase();
        Ok(data
            .tasks
            .into_iter()
            .filter(|t| t.title.to_lowercase() == title)
            .collect())
    }

    fn get_category_by_name(&self, name: &str) -> Result<Option<Category>, StorageError> {
        let data = self.load()?;
        let name = name.to_lowercase();
        Ok(data
            .categories
            .into_iter()
            .find(|c| c.name.to_lowercase() == name))
    }

    fn get_category_id_by_name(&self, name: &str) -> Result<Option<u64>, StorageError> {
        Ok(self.get_category_by_name(name)?.map(|c| c.id))
    }

    fn move_task_to_category(
        &self,
        task_id: u64,
        new_category_id: u64,
    ) -> Result<(), StorageError> {
        let mut data = self.load()?;
        if let Some(task) = data.tasks.iter_mut().find(|t| t.id == task_id) {
            task.category_id = new_category_id;
            task.updated_at = chrono::Utc::now();
            self.save(&data)
        } else {
            Err(StorageError::Storage(format!(
                "Task with id {} not found",
                task_id
            )))
        }
    }

    fn get_tasks_by_category_name(&self, category_name: &str) -> Result<Vec<Task>, StorageError> {
        if let Some(category_id) = self.get_category_id_by_name(category_name)? {
            self.get_tasks_by_category(category_id)
        } else {
            Ok(Vec::new())
        }
    }

    fn get_deleted_tasks(&self) -> Result<Vec<Task>, StorageError> {
        // Category ID 0 is reserved for the "Deleted" category
        self.get_tasks_by_category(0)
    }

    fn soft_delete_task(&self, task_id: u64) -> Result<(), StorageError> {
        // Move task to "Deleted" category (ID 0)
        self.move_task_to_category(task_id, 0)
    }

    fn purge_deleted_tasks(&self, days_threshold: u32) -> Result<(), StorageError> {
        let mut data = self.load()?;
        let now = chrono::Utc::now();
        let threshold = now - chrono::Duration::days(days_threshold as i64);

        // Remove tasks that are older than the threshold
        data.tasks.retain(|t| {
            if t.category_id == 0 {
                // If in deleted category
                t.updated_at > threshold
            } else {
                true
            }
        });

        self.save(&data)
    }

    fn get_all_categories(&self) -> Result<Vec<Category>, StorageError> {
        let data = self.load()?;
        Ok(data.categories)
    }

    fn get_all_tasks(&self) -> Result<Vec<Task>, StorageError> {
        let data = self.load()?;
        Ok(data.tasks)
    }

    fn get_tasks_by_priority_and_category(
        &self,
        priority: Priority,
        category_id: u64,
    ) -> Result<Vec<Task>, StorageError> {
        let data = self.load()?;
        Ok(data
            .tasks
            .into_iter()
            .filter(|t| t.priority == priority && t.category_id == category_id)
            .collect())
    }

    fn get_tasks_by_completion_and_category(
        &self,
        completed: bool,
        category_id: u64,
    ) -> Result<Vec<Task>, StorageError> {
        let data = self.load()?;
        Ok(data
            .tasks
            .into_iter()
            .filter(|t| t.completed == completed && t.category_id == category_id)
            .collect())
    }

    fn get_tasks_by_completion_and_priority(
        &self,
        completed: bool,
        priority: Priority,
    ) -> Result<Vec<Task>, StorageError> {
        let data = self.load()?;
        Ok(data
            .tasks
            .into_iter()
            .filter(|t| t.completed == completed && t.priority == priority)
            .collect())
    }

    fn get_tasks_by_completion_priority_and_category(
        &self,
        completed: bool,
        priority: Priority,
        category_id: u64,
    ) -> Result<Vec<Task>, StorageError> {
        let data = self.load()?;
        Ok(data
            .tasks
            .into_iter()
            .filter(|t| {
                t.completed == completed && t.priority == priority && t.category_id == category_id
            })
            .collect())
    }
}

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
            });
        }

        let contents = std::fs::read_to_string(&self.path)?;
        let data: StorageData = serde_json::from_str(&contents)?;
        Ok(data)
    }
}

#[allow(dead_code)]
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
            let storage = SqliteStorage::new(path)?;
            Ok(Box::new(storage))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        }
    }

    // Helper function to create a test storage with data
    fn create_test_storage() -> (JsonStorage, NamedTempFile) {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = JsonStorage::new(temp_file.path());
        let data = create_test_data();
        storage.save(&data).unwrap();
        (storage, temp_file)
    }

    #[test]
    fn test_basic_storage_operations() {
        let (storage, _temp) = create_test_storage();
        let initial_data = storage.load().unwrap();
        assert_eq!(initial_data.tasks.len(), 4);
        assert_eq!(initial_data.categories.len(), 2);

        // Test add_task
        let new_task = Task {
            id: 5,
            title: "New Task".to_string(),
            category_id: 1,
            completed: false,
            priority: Priority::Medium,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        storage.add_task(new_task.clone()).unwrap();
        let tasks = storage.get_tasks_by_category(1).unwrap();
        assert_eq!(tasks.len(), 3); // Original 2 + new 1

        // Test delete_task
        storage.delete_task(5).unwrap();
        let tasks = storage.get_tasks_by_category(1).unwrap();
        assert_eq!(tasks.len(), 2);

        // Test update_task
        let mut task = storage.get_task(1).unwrap().unwrap();
        task.title = "Updated Task".to_string();
        storage.update_task(task).unwrap();
        let updated_task = storage.get_task(1).unwrap().unwrap();
        assert_eq!(updated_task.title, "Updated Task");

        // Test add_category
        let new_category = Category {
            id: 3,
            name: "New Category".to_string(),
            created_at: Utc::now(),
        };
        storage.add_category(new_category.clone()).unwrap();
        let categories = storage.get_all_categories().unwrap();
        assert_eq!(categories.len(), 3);

        // Test delete_category (empty category)
        storage.delete_category(3).unwrap();
        let categories = storage.get_all_categories().unwrap();
        assert_eq!(categories.len(), 2);
    }

    #[test]
    fn test_name_based_lookups() {
        let (storage, _temp) = create_test_storage();

        // Test get_tasks_by_title
        let tasks = storage.get_tasks_by_title("High Priority Task").unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, 1);

        // Test get_category_by_name
        let category = storage.get_category_by_name("Work").unwrap().unwrap();
        assert_eq!(category.id, 1);

        // Test get_category_id_by_name
        let category_id = storage.get_category_id_by_name("Work").unwrap().unwrap();
        assert_eq!(category_id, 1);

        // Test get_tasks_by_category_name
        let tasks = storage.get_tasks_by_category_name("Work").unwrap();
        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn test_task_movement() {
        let (storage, _temp) = create_test_storage();

        // Test move_task_to_category
        storage.move_task_to_category(1, 2).unwrap();
        let tasks = storage.get_tasks_by_category(2).unwrap();
        assert_eq!(tasks.len(), 2); // Original 1 + moved 1
        assert!(tasks.iter().any(|t| t.id == 1));
    }

    #[test]
    fn test_soft_delete() {
        let (storage, _temp) = create_test_storage();

        // Test get_deleted_tasks
        let deleted_tasks = storage.get_deleted_tasks().unwrap();
        assert_eq!(deleted_tasks.len(), 1);

        // Test soft_delete_task
        storage.soft_delete_task(1).unwrap();
        let deleted_tasks = storage.get_deleted_tasks().unwrap();
        assert_eq!(deleted_tasks.len(), 2);

        // Test purge_deleted_tasks
        storage.purge_deleted_tasks(0).unwrap(); // Purge immediately
        let deleted_tasks = storage.get_deleted_tasks().unwrap();
        assert_eq!(deleted_tasks.len(), 0);
    }

    #[test]
    fn test_filtering() {
        let (storage, _temp) = create_test_storage();

        // Test get_tasks_by_priority
        let high_priority_tasks = storage.get_tasks_by_priority(Priority::High).unwrap();
        assert_eq!(high_priority_tasks.len(), 1);

        // Test get_completed_tasks
        let completed_tasks = storage.get_completed_tasks().unwrap();
        assert_eq!(completed_tasks.len(), 1);

        // Test get_incomplete_tasks
        let incomplete_tasks = storage.get_incomplete_tasks().unwrap();
        assert_eq!(incomplete_tasks.len(), 3); // 3 incomplete tasks (including deleted)

        // Test search_tasks
        let search_results = storage.search_tasks("Priority").unwrap();
        assert_eq!(search_results.len(), 3);

        // Test combined filters
        let tasks = storage
            .get_tasks_by_priority_and_category(Priority::High, 1)
            .unwrap();
        assert_eq!(tasks.len(), 1);

        let tasks = storage
            .get_tasks_by_completion_and_category(true, 1)
            .unwrap();
        assert_eq!(tasks.len(), 1);

        let tasks = storage
            .get_tasks_by_completion_and_priority(true, Priority::Medium)
            .unwrap();
        assert_eq!(tasks.len(), 1);

        let tasks = storage
            .get_tasks_by_completion_priority_and_category(true, Priority::Medium, 1)
            .unwrap();
        assert_eq!(tasks.len(), 1);
    }

    #[test]
    fn test_error_cases() {
        let (storage, _temp) = create_test_storage();

        // Test updating non-existent task
        let non_existent_task = Task {
            id: 999,
            title: "Non-existent".to_string(),
            category_id: 1,
            completed: false,
            priority: Priority::Medium,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        assert!(storage.update_task(non_existent_task).is_err());

        // Test deleting category with tasks
        assert!(storage.delete_category(1).is_err());

        // Test moving non-existent task
        assert!(storage.move_task_to_category(999, 2).is_err());

        // Test getting non-existent category
        let category = storage.get_category(999).unwrap();
        assert!(category.is_none());
    }

    #[test]
    fn test_id_generation() {
        let (storage, _temp) = create_test_storage();

        // Test get_next_task_id
        let next_task_id = storage.get_next_task_id().unwrap();
        assert_eq!(next_task_id, 5); // Since we have tasks 1-4 in test data

        // Test get_next_category_id
        let next_category_id = storage.get_next_category_id().unwrap();
        assert_eq!(next_category_id, 3); // Since we have categories 1-2 in test data
    }
}
