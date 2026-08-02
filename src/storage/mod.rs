use crate::models::{Category, Priority, StorageData, StorageError, Task};
use std::boxed::Box;
use std::path::Path;

pub mod config;
pub mod json;
pub mod sqlite;

#[cfg(test)]
pub mod test_utils;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
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

    /// Returns the lowest unused category ID, starting from 1.
    ///
    /// Unlike `get_next_task_id`, this deliberately fills gaps rather than
    /// always handing out `max + 1`: the README specifies that "when deleting a
    /// category it is removed and its ID is made available again" (issue #16).
    /// ID 0 is never returned - it is reserved for the magic "Uncategorized"
    /// category.
    fn get_next_category_id(&self) -> Result<u64, StorageError> {
        let data = self.load()?;
        let mut used_ids: Vec<u64> = data.categories.iter().map(|c| c.id).collect();
        used_ids.sort_unstable();

        let mut next_id = 1;
        for id in used_ids {
            if id > next_id {
                break;
            }
            if id == next_id {
                next_id = id + 1;
            }
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
pub fn create_storage(
    storage_type: StorageType,
    path: &Path,
) -> Result<Box<dyn Storage>, StorageError> {
    match storage_type {
        StorageType::Json => {
            let storage = json::JsonStorage::new(path);
            Ok(Box::new(storage))
        }
        StorageType::Sqlite => {
            let storage = sqlite::SqliteStorage::new(path)?;
            Ok(Box::new(storage))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_storage_creation() {
        // Each backend gets its own independent temp file. Pointing both backends
        // at the *same* path (as this test used to do) doesn't verify backend
        // interoperability - it just happens to pass because JsonStorage::load()
        // never writes, leaving the file at zero length, and an empty file is a
        // valid (empty) SQLite database. See test_sqlite_rejects_json_populated_file
        // below for the actual cross-backend scenario.
        let json_temp_file = NamedTempFile::new().unwrap();
        let json_storage = create_storage(StorageType::Json, json_temp_file.path()).unwrap();
        assert!(json_storage.load().is_ok());

        let sqlite_temp_file = NamedTempFile::new().unwrap();
        let sqlite_storage = create_storage(StorageType::Sqlite, sqlite_temp_file.path()).unwrap();
        assert!(sqlite_storage.load().is_ok());
    }

    /// Encodes the requirement from issue #17: switching storage backends against
    /// a file that already holds data in a *different* backend's format must fail
    /// gracefully with a typed `StorageError`, not panic and not silently succeed
    /// with wrong/empty data.
    ///
    /// NOTE: for this specific direction (SQLite opened against a file already
    /// populated by the JSON backend), this currently passes: SQLite's own
    /// on-disk header check inside `execute_batch` rejects the file with
    /// "file is not a database" before any table is created, which
    /// `SqliteStorage::new` surfaces as `StorageError::Storage`, not a panic and
    /// not silent success/corruption (verified: the original JSON file is left
    /// byte-for-byte intact and still loads correctly afterwards). This is not a
    /// deliberate validation in this codebase, though - it's an accidental
    /// byproduct of the SQLite file format having a magic header. There is no
    /// equivalent protection in the *other* direction (JSON backend silently
    /// overwriting an existing SQLite file via `std::fs::write` with no format
    /// check at all), and there is no format check at all for two files that
    /// both happen to satisfy their respective format's parser. #17 should stay
    /// open for that gap; this test just pins down the one direction that
    /// already behaves correctly today so a future regression is caught.
    #[test]
    fn test_sqlite_rejects_json_populated_file() {
        let temp_file = NamedTempFile::new().unwrap();

        // Populate the file with real data through the JSON backend.
        let json_storage = create_storage(StorageType::Json, temp_file.path()).unwrap();
        let mut data = StorageData::new();
        let category = Category::new("Work".to_string(), None).unwrap();
        data.categories.push(category.clone());
        let task = Task::new("Test task".to_string(), category.id, None, Priority::Medium).unwrap();
        data.tasks.push(task);
        json_storage.save(&data).unwrap();

        // Pointing SQLite storage at a file that already holds non-SQLite (JSON)
        // data must be rejected gracefully with a typed StorageError - never a
        // panic, and never a silent success that hides or discards the existing
        // data.
        let result = create_storage(StorageType::Sqlite, temp_file.path());
        assert!(
            result.is_err(),
            "expected a graceful StorageError when opening a JSON-populated file as SQLite, got Ok(..)"
        );
    }
}
