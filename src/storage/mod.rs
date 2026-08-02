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

    // Deliberately does NOT filter out soft-deleted tasks: `restore` and
    // `purge_deleted_tasks`/callers that need to inspect a specific deleted
    // task by ID still need to be able to reach it here.
    fn get_task(&self, task_id: u64) -> Result<Option<Task>, StorageError> {
        let data = self.load()?;
        Ok(data.tasks.into_iter().find(|t| t.id == task_id))
    }

    fn add_category(&self, category: Category) -> Result<(), StorageError> {
        let mut data = self.load()?;
        data.categories.push(category);
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

    /// Loads all tasks, excluding soft-deleted ones. Every listing/search/
    /// filter helper below builds on this rather than repeating the
    /// `deleted_at.is_none()` check, so a deleted task can never resurface in
    /// a query by accident - it stays invisible until `restore`d or purged.
    /// `get_task` and `get_next_task_id` intentionally do NOT use this - see
    /// their own comments.
    fn live_tasks(&self) -> Result<Vec<Task>, StorageError> {
        let data = self.load()?;
        Ok(data
            .tasks
            .into_iter()
            .filter(|t| t.deleted_at.is_none())
            .collect())
    }

    fn get_tasks_by_category(&self, category_id: u64) -> Result<Vec<Task>, StorageError> {
        Ok(self
            .live_tasks()?
            .into_iter()
            .filter(|t| t.category_id == category_id)
            .collect())
    }

    fn get_tasks_by_priority(&self, priority: Priority) -> Result<Vec<Task>, StorageError> {
        Ok(self
            .live_tasks()?
            .into_iter()
            .filter(|t| t.priority == priority)
            .collect())
    }

    fn get_completed_tasks(&self) -> Result<Vec<Task>, StorageError> {
        Ok(self
            .live_tasks()?
            .into_iter()
            .filter(|t| t.completed)
            .collect())
    }

    fn get_incomplete_tasks(&self) -> Result<Vec<Task>, StorageError> {
        Ok(self
            .live_tasks()?
            .into_iter()
            .filter(|t| !t.completed)
            .collect())
    }

    fn search_tasks(&self, query: &str) -> Result<Vec<Task>, StorageError> {
        let query = query.to_lowercase();
        Ok(self
            .live_tasks()?
            .into_iter()
            .filter(|t| t.title.to_lowercase().contains(&query))
            .collect())
    }

    // Deliberately keyed off *all* tasks (including soft-deleted ones), not
    // `live_tasks`: a deleted task still holds its ID until it is purged, so
    // handing that ID to a new task would collide once the deleted one is
    // restored or looked up.
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
        let title = title.to_lowercase();
        Ok(self
            .live_tasks()?
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

    /// Tasks that have been soft-deleted (`Task::soft_delete`), regardless of
    /// which real category they belong to.
    ///
    /// This used to be implemented as "tasks in category 0", back when ID 0
    /// doubled as a magic "Deleted" category. That collided with ID 0 also
    /// meaning "Uncategorized" (`category_manager::UNCATEGORIZED_ID`):
    /// deleting a *category* reassigns its tasks to 0, which would make them
    /// show up here as deleted and be destroyed by a purge the user never
    /// asked for. Issue #29 fixed this by keying deletion off `deleted_at`
    /// instead of `category_id`, so category deletion and task deletion can
    /// no longer be confused with each other.
    fn get_deleted_tasks(&self) -> Result<Vec<Task>, StorageError> {
        let data = self.load()?;
        Ok(data
            .tasks
            .into_iter()
            .filter(|t| t.deleted_at.is_some())
            .collect())
    }

    fn soft_delete_task(&self, task_id: u64) -> Result<(), StorageError> {
        let mut data = self.load()?;
        if let Some(task) = data.tasks.iter_mut().find(|t| t.id == task_id) {
            task.soft_delete();
            self.save(&data)
        } else {
            Err(StorageError::Storage(format!(
                "Task with id {} not found",
                task_id
            )))
        }
    }

    /// Permanently removes tasks that were soft-deleted more than
    /// `days_threshold` days ago (README: `deleted-task-lifespan`).
    ///
    /// A threshold of 0 means "never automatically delete" per the README's
    /// config table, so it short-circuits here rather than falling through
    /// to a zero-day cutoff that would purge every deleted task immediately.
    ///
    /// The cutoff is judged strictly by `deleted_at`, never `updated_at`:
    /// editing a soft-deleted task (e.g. during a `restore` that fails
    /// partway, or any future feature that touches a deleted task) must not
    /// reset its purge clock.
    ///
    /// Writes back only when something was actually purged. This sweep runs
    /// once per invocation from `main::open_storage`, so saving
    /// unconditionally would make read-only commands like `list` rewrite the
    /// whole data file on every run whenever a non-zero lifespan is
    /// configured.
    fn purge_deleted_tasks(&self, days_threshold: u32) -> Result<(), StorageError> {
        if days_threshold == 0 {
            return Ok(());
        }

        let mut data = self.load()?;
        let cutoff = chrono::Utc::now() - chrono::Duration::days(days_threshold as i64);

        let before = data.tasks.len();
        data.tasks.retain(|t| match t.deleted_at {
            Some(deleted_at) => deleted_at > cutoff,
            None => true,
        });

        if data.tasks.len() == before {
            return Ok(());
        }

        self.save(&data)
    }

    /// Unconditionally and permanently removes *every* currently soft-deleted
    /// task, no matter how recently it was deleted. This is the primitive
    /// behind the manual `deleted flush` command (README: "Remove all
    /// deleted items") - an explicit, one-shot user action.
    ///
    /// Do NOT confuse this with `purge_deleted_tasks` above:
    ///   - `purge_deleted_tasks(days_threshold)` is the *automatic*,
    ///     age-gated sweep driven by `deleted-task-lifespan`, where a
    ///     threshold of `0` means "never purge anything" (the documented
    ///     default).
    ///   - `purge_all_deleted_tasks` (this method) takes no threshold at all
    ///     and always empties out every soft-deleted task. Calling
    ///     `purge_deleted_tasks(0)` to implement a manual flush would be
    ///     exactly backwards - there, `0` means "purge nothing".
    ///
    /// Returns how many tasks were actually removed, so callers (the CLI)
    /// can report it back to the user.
    fn purge_all_deleted_tasks(&self) -> Result<usize, StorageError> {
        let mut data = self.load()?;
        let before = data.tasks.len();
        data.tasks.retain(|t| t.deleted_at.is_none());
        let purged = before - data.tasks.len();

        // Same reasoning as `purge_deleted_tasks`: a flush with nothing to
        // purge is a documented no-op, so don't rewrite the data file for it.
        if purged > 0 {
            self.save(&data)?;
        }

        Ok(purged)
    }

    fn get_all_categories(&self) -> Result<Vec<Category>, StorageError> {
        let data = self.load()?;
        Ok(data.categories)
    }

    fn get_all_tasks(&self) -> Result<Vec<Task>, StorageError> {
        self.live_tasks()
    }

    fn get_tasks_by_priority_and_category(
        &self,
        priority: Priority,
        category_id: u64,
    ) -> Result<Vec<Task>, StorageError> {
        Ok(self
            .live_tasks()?
            .into_iter()
            .filter(|t| t.priority == priority && t.category_id == category_id)
            .collect())
    }

    fn get_tasks_by_completion_and_category(
        &self,
        completed: bool,
        category_id: u64,
    ) -> Result<Vec<Task>, StorageError> {
        Ok(self
            .live_tasks()?
            .into_iter()
            .filter(|t| t.completed == completed && t.category_id == category_id)
            .collect())
    }

    fn get_tasks_by_completion_and_priority(
        &self,
        completed: bool,
        priority: Priority,
    ) -> Result<Vec<Task>, StorageError> {
        Ok(self
            .live_tasks()?
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
        Ok(self
            .live_tasks()?
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
    use chrono::Utc;
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

    /// Issue #29 regression test: deleting a *category* reassigns its tasks
    /// to the magic "Uncategorized" ID (`category_manager::UNCATEGORIZED_ID`,
    /// which is 0). Before this fix, `get_deleted_tasks` treated
    /// `category_id == 0` as "this task is deleted", so a task merely
    /// orphaned by deleting its category would be misreported as deleted -
    /// and a subsequent `purge_deleted_tasks` flush would destroy it, even
    /// though the user never asked to delete that task. This is the
    /// data-loss scenario #29 exists to close off.
    #[test]
    fn test_category_deletion_does_not_orphan_tasks_into_deleted() {
        use crate::category_manager::{CategoryManager, UNCATEGORIZED_ID};
        use crate::storage::test_utils::TestStorage;

        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let mut manager = CategoryManager::new(storage);

        let category_id = manager
            .add_category("Work".to_string(), None)
            .expect("Failed to add category");

        let mut task = Task::new(
            "Finish report".to_string(),
            category_id,
            None,
            Priority::Medium,
        )
        .unwrap();
        task.id = storage.get_next_task_id().unwrap();
        storage.add_task(task).unwrap();

        // Delete the category without specifying a destination: its tasks
        // fall back to Uncategorized (ID 0), not deletion.
        manager.delete_category(category_id, None).unwrap();

        let tasks = storage.get_all_tasks().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].category_id, UNCATEGORIZED_ID);
        assert!(!tasks[0].is_deleted());

        let deleted = storage.get_deleted_tasks().unwrap();
        assert!(
            deleted.is_empty(),
            "task orphaned by category deletion must not appear in get_deleted_tasks (issue #29)"
        );
    }

    /// `soft_delete_task` must retain the task's real category (that's what
    /// makes restoring it lossless) while still hiding it from every listing/
    /// search helper and surfacing it via `get_deleted_tasks`.
    #[test]
    fn test_soft_delete_task_preserves_category_and_hides_from_queries() {
        use crate::storage::test_utils::TestStorage;

        let test_storage = TestStorage::new();
        let storage = test_storage.storage();

        // The category must have a real, non-zero ID for this test to mean
        // anything: `Category::new` leaves `id` at 0, which is also
        // `UNCATEGORIZED_ID`, so the old "move the task to category 0"
        // implementation of soft delete would satisfy the category assertion
        // below by coincidence rather than by preserving anything.
        let mut category = Category::new("Work".to_string(), None).unwrap();
        category.id = storage.get_next_category_id().unwrap();
        let category_id = category.id;
        assert_ne!(category_id, 0);
        storage.add_category(category).unwrap();

        let mut task = Task::new(
            "Finish report".to_string(),
            category_id,
            None,
            Priority::Medium,
        )
        .unwrap();
        task.id = storage.get_next_task_id().unwrap();
        let task_id = task.id;
        storage.add_task(task).unwrap();

        storage.soft_delete_task(task_id).unwrap();

        let deleted = storage.get_deleted_tasks().unwrap();
        assert_eq!(deleted.len(), 1);
        assert_eq!(
            deleted[0].category_id, category_id,
            "soft delete must leave the task in its real category"
        );
        assert!(deleted[0].deleted_at.is_some());

        assert!(storage
            .get_tasks_by_category(category_id)
            .unwrap()
            .is_empty());
        assert!(storage.search_tasks("report").unwrap().is_empty());
        assert!(storage.get_all_tasks().unwrap().is_empty());

        // get_task is the one exception: it must still be reachable by ID so
        // restore/purge can find it.
        let fetched = storage.get_task(task_id).unwrap();
        assert!(fetched.is_some());
        assert!(fetched.unwrap().is_deleted());
    }

    /// README: "A value of 0, the default, indicates they are never
    /// automatically deleted." A threshold of 0 must not purge anything,
    /// even a task deleted long ago.
    #[test]
    fn test_purge_deleted_tasks_zero_threshold_purges_nothing() {
        use crate::storage::test_utils::TestStorage;

        let test_storage = TestStorage::new();
        let storage = test_storage.storage();

        let mut task = Task::new("Ancient task".to_string(), 0, None, Priority::Low).unwrap();
        task.id = storage.get_next_task_id().unwrap();
        task.deleted_at = Some(Utc::now() - chrono::Duration::days(3650));
        storage.add_task(task).unwrap();

        storage.purge_deleted_tasks(0).unwrap();

        assert_eq!(storage.get_deleted_tasks().unwrap().len(), 1);
    }

    /// `purge_deleted_tasks(n)` must key strictly off `deleted_at`, dropping
    /// tasks deleted more than `n` days ago and keeping recently-deleted
    /// ones. Crucially, a task's `updated_at` must have no bearing on this -
    /// that decoupling was the second bug named in issue #29 (editing a
    /// soft-deleted task used to silently reset its purge clock, because the
    /// old implementation judged `updated_at` instead of `deleted_at`).
    #[test]
    fn test_purge_deleted_tasks_keys_off_deleted_at_not_updated_at() {
        use crate::storage::test_utils::TestStorage;

        let test_storage = TestStorage::new();
        let storage = test_storage.storage();

        let now = Utc::now();

        let mut old_task =
            Task::new("Old deleted task".to_string(), 0, None, Priority::Low).unwrap();
        old_task.id = storage.get_next_task_id().unwrap();
        old_task.deleted_at = Some(now - chrono::Duration::days(10));
        // Recently touched, but that must not matter: it was deleted 10 days
        // ago, past a 5-day threshold.
        old_task.updated_at = now;
        let old_task_id = old_task.id;
        storage.add_task(old_task).unwrap();

        let mut recent_task =
            Task::new("Recently deleted task".to_string(), 0, None, Priority::Low).unwrap();
        recent_task.id = storage.get_next_task_id().unwrap();
        recent_task.deleted_at = Some(now - chrono::Duration::days(1));
        // Stale `updated_at`, but that must not matter either: it was only
        // deleted yesterday, well within the 5-day threshold.
        recent_task.updated_at = now - chrono::Duration::days(365);
        let recent_task_id = recent_task.id;
        storage.add_task(recent_task).unwrap();

        storage.purge_deleted_tasks(5).unwrap();

        assert!(storage.get_task(old_task_id).unwrap().is_none());
        let remaining = storage.get_task(recent_task_id).unwrap();
        assert!(remaining.is_some());
        assert!(remaining.unwrap().is_deleted());
    }

    /// The automatic sweep runs on *every* invocation (see
    /// `main::open_storage`), so when there is nothing overdue it must leave
    /// the data file completely alone rather than rewriting it. Otherwise a
    /// read-only `trtodo list` would rewrite storage on every run whenever a
    /// non-zero `deleted-task-lifespan` is configured.
    #[test]
    fn test_purge_deleted_tasks_does_not_rewrite_storage_when_nothing_is_due() {
        use crate::storage::test_utils::TestStorage;

        let test_storage = TestStorage::new();
        let storage = test_storage.storage();

        let mut task = Task::new("Recently deleted".to_string(), 0, None, Priority::Low).unwrap();
        task.id = storage.get_next_task_id().unwrap();
        task.deleted_at = Some(Utc::now() - chrono::Duration::days(1));
        storage.add_task(task).unwrap();

        let path = test_storage.path();
        let before = std::fs::read_to_string(path).unwrap();

        // Deleted yesterday, threshold 30 days: nothing is due.
        storage.purge_deleted_tasks(30).unwrap();

        let after = std::fs::read_to_string(path).unwrap();
        assert_eq!(
            before, after,
            "a sweep with nothing due must not rewrite the data file"
        );

        // A flush that purges nothing is likewise a no-op on disk.
        let test_storage2 = TestStorage::new();
        let storage2 = test_storage2.storage();
        let mut live = Task::new("Live".to_string(), 0, None, Priority::Low).unwrap();
        live.id = storage2.get_next_task_id().unwrap();
        storage2.add_task(live).unwrap();
        let before = std::fs::read_to_string(test_storage2.path()).unwrap();
        assert_eq!(storage2.purge_all_deleted_tasks().unwrap(), 0);
        let after = std::fs::read_to_string(test_storage2.path()).unwrap();
        assert_eq!(before, after);
    }

    /// `purge_all_deleted_tasks` (the `deleted flush` primitive) must remove
    /// every soft-deleted task unconditionally - including ones deleted only
    /// moments ago - unlike `purge_deleted_tasks`, which gates on age and
    /// treats a `0` threshold as "purge nothing". It must also leave live
    /// tasks completely untouched and report an accurate count.
    #[test]
    fn test_purge_all_deleted_tasks_removes_everything_deleted_regardless_of_age() {
        use crate::storage::test_utils::TestStorage;

        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let now = Utc::now();

        let mut just_deleted =
            Task::new("Just deleted".to_string(), 0, None, Priority::Low).unwrap();
        just_deleted.id = storage.get_next_task_id().unwrap();
        just_deleted.deleted_at = Some(now);
        let just_deleted_id = just_deleted.id;
        storage.add_task(just_deleted).unwrap();

        let mut long_deleted =
            Task::new("Long deleted".to_string(), 0, None, Priority::Low).unwrap();
        long_deleted.id = storage.get_next_task_id().unwrap();
        long_deleted.deleted_at = Some(now - chrono::Duration::days(3650));
        let long_deleted_id = long_deleted.id;
        storage.add_task(long_deleted).unwrap();

        let mut live_task = Task::new("Still alive".to_string(), 0, None, Priority::Low).unwrap();
        live_task.id = storage.get_next_task_id().unwrap();
        let live_task_id = live_task.id;
        storage.add_task(live_task).unwrap();

        let purged = storage.purge_all_deleted_tasks().unwrap();
        assert_eq!(purged, 2);

        assert!(storage.get_task(just_deleted_id).unwrap().is_none());
        assert!(storage.get_task(long_deleted_id).unwrap().is_none());
        assert!(storage.get_task(live_task_id).unwrap().is_some());

        // Idempotent: nothing left to purge on a second call.
        assert_eq!(storage.purge_all_deleted_tasks().unwrap(), 0);
    }
}
