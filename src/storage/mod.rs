use crate::models::{Category, Priority, StorageData, StorageError, Task};
use std::path::Path;

pub mod config;
pub mod json;
pub mod sqlite;
pub mod test_utils;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StorageType {
    Json,
    Sqlite,
}

#[allow(dead_code)]
pub trait Storage {
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
        let mut used_ids: Vec<u64> = data.categories.iter().map(|c| c.id).collect();
        used_ids.sort();

        // Find first available ID
        let mut next_id = 1;
        for id in used_ids {
            if id > next_id {
                break;
            }
            next_id = id + 1;
        }

        Ok(next_id)
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
pub fn create_storage(path: &Path) -> Result<Box<dyn Storage>, StorageError> {
    let mut config = crate::config::Config::default();
    config.storage_path = Some(path.to_str().unwrap().to_string());

    match config.storage_type.as_deref().unwrap_or("json") {
        "json" => {
            let storage = json::JsonStorage::new(config)?;
            Ok(Box::new(storage))
        }
        "sqlite" => {
            let storage = sqlite::SqliteStorage::new(config)?;
            Ok(Box::new(storage))
        }
        _ => {
            let storage = json::JsonStorage::new(config)?;
            Ok(Box::new(storage))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::test_utils::create_test_config_manager;

    #[test]
    fn test_storage_manager_default() {
        let (_manager, _temp_dir) = create_test_config_manager();
        let storage = create_storage(Path::new("test.json")).unwrap();
        assert!(storage.load().is_ok());
    }

    #[test]
    fn test_storage_manager_json() {
        let (_manager, _temp_dir) = create_test_config_manager();
        let storage = create_storage(Path::new("test.json")).unwrap();
        assert!(storage.load().is_ok());
    }

    #[test]
    fn test_storage_manager_sqlite() {
        let (_manager, _temp_dir) = create_test_config_manager();
        let storage = create_storage(Path::new("test.db")).unwrap();

        // Initialize with empty data
        let data = StorageData::new();
        storage.save(&data).expect("Failed to initialize storage");

        assert!(storage.load().is_ok());
    }
}
