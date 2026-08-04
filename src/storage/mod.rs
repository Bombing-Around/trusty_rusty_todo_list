use crate::models::{Category, Priority, StorageData, StorageError, Task};
use std::boxed::Box;
use std::path::Path;

pub mod config;
pub mod json;
pub mod sqlite;

#[cfg(test)]
pub mod test_utils;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageType {
    Json,
    Sqlite,
}

impl StorageType {
    /// Parses the `storage.type` config value. Returns `None` for anything
    /// that isn't a backend this build knows about, leaving it to the caller
    /// to decide whether that is an error (`main::open_storage`) or simply
    /// "not my business" (`main`'s pre-`set` migration hook, which lets
    /// `ConfigManager::set`'s own validation produce the message).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "json" => Some(StorageType::Json),
            "sqlite" => Some(StorageType::Sqlite),
            _ => None,
        }
    }

    /// The `storage.type` config value this backend is selected by. Kept
    /// alongside `parse` so the two can't drift apart.
    pub fn as_str(self) -> &'static str {
        match self {
            StorageType::Json => "json",
            StorageType::Sqlite => "sqlite",
        }
    }

    /// The name of the data file this backend keeps inside the `storage.path`
    /// *directory*.
    ///
    /// Every backend gets its own file name on purpose. Pointing two backends
    /// at one path is how the original "file is not a database" panic
    /// happened, and how `JsonStorage::save` (a bare `std::fs::write`, with no
    /// format check) could silently clobber a SQLite database. Distinct names
    /// make that structurally impossible rather than merely unlikely - at the
    /// cost that switching `storage.type` leaves the old backend's file
    /// sitting there unread, which is what `migrate_storage` below exists to
    /// deal with.
    pub fn data_file_name(self) -> &'static str {
        match self {
            StorageType::Json => "trtodo-data.json",
            StorageType::Sqlite => "trtodo-data.db",
        }
    }
}

pub trait Storage {
    fn save(&self, data: &StorageData) -> Result<(), StorageError>;
    fn load(&self) -> Result<StorageData, StorageError>;

    // Convenience methods for common operations
    fn add_task(&self, task: Task) -> Result<(), StorageError> {
        let mut data = self.load()?;
        data.tasks.push(task);
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
        Ok(data.tasks.into_iter().filter(|t| !t.is_deleted()).collect())
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
    /// category it is removed and its ID is made available again".
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

    /// Tasks that have been soft-deleted (`Task::soft_delete`), regardless of
    /// which real category they belong to.
    ///
    /// This used to be implemented as "tasks in category 0", back when ID 0
    /// doubled as a magic "Deleted" category. That collided with ID 0 also
    /// meaning "Uncategorized" (`category_manager::UNCATEGORIZED_ID`):
    /// deleting a *category* reassigns its tasks to 0, which would make them
    /// show up here as deleted and be destroyed by a purge the user never
    /// asked for. Keying deletion off `deleted_at` instead of
    /// `category_id` fixed that, so category deletion and task deletion can
    /// no longer be confused with each other.
    fn get_deleted_tasks(&self) -> Result<Vec<Task>, StorageError> {
        let data = self.load()?;
        Ok(data.tasks.into_iter().filter(|t| t.is_deleted()).collect())
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
}

/// What `migrate_storage` actually did, so the caller can tell the user the
/// truth instead of printing an unconditional "may require data migration"
/// warning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationOutcome {
    /// The source store holds no tasks and no categories, so there was
    /// nothing to carry over. The common case on a first-ever switch.
    SourceEmpty,
    /// The destination store already holds data of its own. Nothing was
    /// copied and, crucially, nothing was overwritten - the user is switching
    /// back to a backend they had already been using.
    DestinationNotEmpty { tasks: usize, categories: usize },
    /// The source's contents were copied into the (previously empty)
    /// destination. The source is left exactly as it was.
    Migrated { tasks: usize, categories: usize },
}

/// True when a store holds nothing a user would recognise as their data.
///
/// Deliberately judged on tasks and categories only. `current_category` is
/// not considered: a context can only ever point at a category that exists,
/// so with no categories it is at best stale, and treating it as "content"
/// would block a perfectly good migration.
fn is_empty(data: &StorageData) -> bool {
    data.tasks.is_empty() && data.categories.is_empty()
}

/// Copies everything in `source` into `destination`, for when the user
/// changes `storage.type` and would otherwise watch all their tasks and
/// categories vanish.
///
/// Backend-agnostic on purpose: it speaks only `Storage`, so json -> sqlite
/// and sqlite -> json are the same code path, and any future backend gets the
/// behaviour for free.
///
/// The rules it holds to, in order:
///
///   - **Never destructive.** The source is only ever `load`ed, never written
///     to and never removed. After a migration both stores hold the data, and
///     switching `storage.type` back is enough to reach the original.
///   - **Never clobbers a non-empty destination.** If the destination already
///     has tasks or categories - the "I'm switching back to the backend I
///     used last week" case - this reports `DestinationNotEmpty` and writes
///     nothing at all. Silently overwriting it would be exactly the data loss
///     this function exists to prevent.
///   - **IDs are preserved verbatim, never renumbered.** That is only sound
///     because of the rule above: the destination is empty, so there is
///     nothing for the source's IDs to collide with. This is also why no
///     merge is attempted - reconciling two populated stores would mean
///     renumbering tasks and categories, silently invalidating every ID the
///     user has memorised or scripted against. Refusing is the coherent
///     answer; a merge would need its own design and its own issue.
///
/// `current_category` rides along, so the user's `category use` context
/// survives the switch. The vestigial `config` blob does not: it belongs to
/// whichever file it is written in (see `StorageData::new`), so the
/// destination keeps its own.
pub fn migrate_storage(
    source: &dyn Storage,
    destination: &dyn Storage,
) -> Result<MigrationOutcome, StorageError> {
    let source_data = source.load()?;
    if is_empty(&source_data) {
        return Ok(MigrationOutcome::SourceEmpty);
    }

    let destination_data = destination.load()?;
    if !is_empty(&destination_data) {
        return Ok(MigrationOutcome::DestinationNotEmpty {
            tasks: destination_data.tasks.len(),
            categories: destination_data.categories.len(),
        });
    }

    let tasks = source_data.tasks.len();
    let categories = source_data.categories.len();

    destination.save(&StorageData {
        version: destination_data.version,
        tasks: source_data.tasks,
        categories: source_data.categories,
        config: destination_data.config,
        current_category: source_data.current_category,
        last_sync: chrono::Utc::now(),
    })?;

    Ok(MigrationOutcome::Migrated { tasks, categories })
}

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
    use tempfile::{NamedTempFile, TempDir};

    /// A category and a task belonging to it, with explicit non-zero IDs so
    /// migration tests can assert the IDs survived rather than coincidentally
    /// matching a default of 0.
    fn populated_data() -> StorageData {
        let mut data = StorageData::new();

        let mut work = Category::new("Work".to_string(), None).unwrap();
        work.id = 4;
        let mut personal = Category::new("Personal".to_string(), None).unwrap();
        personal.id = 9;

        let mut report = Task::new("Finish report".to_string(), work.id, None, Priority::High)
            .expect("valid task");
        report.id = 12;
        let mut groceries = Task::new("Buy milk".to_string(), personal.id, None, Priority::Low)
            .expect("valid task");
        groceries.id = 30;

        data.categories.push(work);
        data.categories.push(personal);
        data.tasks.push(report);
        data.tasks.push(groceries);
        data.current_category = Some(9);

        data
    }

    /// The headline behaviour: switching backends must carry the
    /// user's data across, not silently strand it in a file nothing reads
    /// anymore. Exercised across *real* backends (JSON -> SQLite), since the
    /// whole point is that the two formats are involved.
    #[test]
    fn test_migrate_storage_carries_data_across_backends() {
        let dir = TempDir::new().unwrap();
        let json_path = dir.path().join("trtodo-data.json");
        let sqlite_path = dir.path().join("trtodo-data.db");

        let source = create_storage(StorageType::Json, &json_path).unwrap();
        source.save(&populated_data()).unwrap();

        let destination = create_storage(StorageType::Sqlite, &sqlite_path).unwrap();
        let outcome = migrate_storage(&*source, &*destination).unwrap();
        assert_eq!(
            outcome,
            MigrationOutcome::Migrated {
                tasks: 2,
                categories: 2
            }
        );

        let migrated = destination.load().unwrap();
        assert_eq!(migrated.tasks.len(), 2);
        assert_eq!(migrated.categories.len(), 2);

        // IDs are carried over verbatim - a user's memorised/scripted task IDs
        // must not be renumbered by a backend switch.
        let mut task_ids: Vec<u64> = migrated.tasks.iter().map(|t| t.id).collect();
        task_ids.sort_unstable();
        assert_eq!(task_ids, vec![12, 30]);
        let mut category_ids: Vec<u64> = migrated.categories.iter().map(|c| c.id).collect();
        category_ids.sort_unstable();
        assert_eq!(category_ids, vec![4, 9]);

        // ... as is the `category use` context.
        assert_eq!(migrated.current_category, Some(9));

        // Non-destructive: the source still holds everything it did, and the
        // file is still there to be switched back to.
        assert!(json_path.exists());
        let source_after = source.load().unwrap();
        assert_eq!(source_after.tasks.len(), 2);
        assert_eq!(source_after.categories.len(), 2);
        assert_eq!(source_after.current_category, Some(9));
    }

    /// The same must hold in the other direction - `migrate_storage` speaks
    /// only the `Storage` trait, and this pins that it really is symmetric
    /// rather than accidentally JSON-shaped.
    #[test]
    fn test_migrate_storage_carries_data_back_from_sqlite_to_json() {
        let dir = TempDir::new().unwrap();
        let source =
            create_storage(StorageType::Sqlite, &dir.path().join("trtodo-data.db")).unwrap();
        source.save(&populated_data()).unwrap();

        let destination =
            create_storage(StorageType::Json, &dir.path().join("trtodo-data.json")).unwrap();
        assert_eq!(
            migrate_storage(&*source, &*destination).unwrap(),
            MigrationOutcome::Migrated {
                tasks: 2,
                categories: 2
            }
        );

        let migrated = destination.load().unwrap();
        assert_eq!(migrated.tasks.len(), 2);
        assert_eq!(migrated.categories.len(), 2);
        assert_eq!(migrated.current_category, Some(9));
    }

    /// A destination that already holds the user's data must never be
    /// overwritten - that would turn a confusing-but-recoverable backend
    /// switch into real data loss. This is the "switching back to the backend
    /// I used last week" case.
    #[test]
    fn test_migrate_storage_never_clobbers_a_non_empty_destination() {
        let dir = TempDir::new().unwrap();
        let json_path = dir.path().join("trtodo-data.json");
        let sqlite_path = dir.path().join("trtodo-data.db");

        let source = create_storage(StorageType::Sqlite, &sqlite_path).unwrap();
        source.save(&populated_data()).unwrap();

        // The destination has one category and no tasks of its own.
        let destination = create_storage(StorageType::Json, &json_path).unwrap();
        let mut existing = StorageData::new();
        let mut kept = Category::new("Already here".to_string(), None).unwrap();
        kept.id = 1;
        existing.categories.push(kept);
        destination.save(&existing).unwrap();
        let before = std::fs::read_to_string(&json_path).unwrap();

        assert_eq!(
            migrate_storage(&*source, &*destination).unwrap(),
            MigrationOutcome::DestinationNotEmpty {
                tasks: 0,
                categories: 1
            }
        );

        // Nothing written at all, not even a re-serialization of what was
        // already there.
        assert_eq!(std::fs::read_to_string(&json_path).unwrap(), before);
        let destination_after = destination.load().unwrap();
        assert_eq!(destination_after.categories.len(), 1);
        assert_eq!(destination_after.categories[0].name, "Already here");
        assert!(destination_after.tasks.is_empty());

        // And the source is, as always, untouched.
        assert_eq!(source.load().unwrap().tasks.len(), 2);
    }

    /// A first-ever switch has nothing to carry over. That must be a silent
    /// no-op rather than a write (or a scary warning) - the destination file
    /// should not even be materialised with content.
    #[test]
    fn test_migrate_storage_reports_an_empty_source_without_writing() {
        let dir = TempDir::new().unwrap();
        let json_path = dir.path().join("trtodo-data.json");
        let sqlite_path = dir.path().join("trtodo-data.db");

        let source = create_storage(StorageType::Json, &json_path).unwrap();
        let destination = create_storage(StorageType::Sqlite, &sqlite_path).unwrap();

        assert_eq!(
            migrate_storage(&*source, &*destination).unwrap(),
            MigrationOutcome::SourceEmpty
        );
        assert!(destination.load().unwrap().tasks.is_empty());
        assert!(destination.load().unwrap().categories.is_empty());
    }

    /// Soft-deleted tasks are still the user's data (they can be restored, and
    /// `deleted flush` reports them), so they must ride along with everything
    /// else rather than being quietly dropped by the switch.
    #[test]
    fn test_migrate_storage_carries_soft_deleted_tasks_too() {
        let dir = TempDir::new().unwrap();
        let source =
            create_storage(StorageType::Json, &dir.path().join("trtodo-data.json")).unwrap();

        let mut data = populated_data();
        data.tasks[0].soft_delete();
        source.save(&data).unwrap();

        let destination =
            create_storage(StorageType::Sqlite, &dir.path().join("trtodo-data.db")).unwrap();
        assert_eq!(
            migrate_storage(&*source, &*destination).unwrap(),
            MigrationOutcome::Migrated {
                tasks: 2,
                categories: 2
            }
        );

        assert_eq!(destination.get_deleted_tasks().unwrap().len(), 1);
        assert_eq!(destination.live_tasks().unwrap().len(), 1);
    }

    /// The file names are what keep the two backends from ever being pointed
    /// at the same path (see `StorageType::data_file_name`), so they are worth
    /// pinning: making them equal would silently re-open the original
    /// "file is not a database" failure.
    #[test]
    fn test_each_backend_has_its_own_data_file_name() {
        assert_ne!(
            StorageType::Json.data_file_name(),
            StorageType::Sqlite.data_file_name()
        );
        assert_eq!(StorageType::parse("json"), Some(StorageType::Json));
        assert_eq!(StorageType::parse("sqlite"), Some(StorageType::Sqlite));
        assert_eq!(StorageType::parse("postgres"), None);
        assert_eq!(StorageType::Json.as_str(), "json");
        assert_eq!(StorageType::Sqlite.as_str(), "sqlite");
    }

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

    /// Switching storage backends against
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
    /// byproduct of the SQLite file format having a magic header. There is still
    /// no equivalent protection in the *other* direction (JSON backend silently
    /// overwriting an existing SQLite file via `std::fs::write` with no format
    /// check at all), and there is no format check at all for two files that
    /// both happen to satisfy their respective format's parser.
    ///
    /// That gap is no longer *reachable through configuration*: each backend
    /// now owns a distinct file name inside the `storage.path` directory (see
    /// `StorageType::data_file_name`, pinned by
    /// `test_each_backend_has_its_own_data_file_name`), so the two can't be
    /// aimed at one file. This test still pins the one direction that behaves
    /// correctly at the storage layer itself, for anyone who constructs the
    /// backends directly.
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

    /// The mirror-image direction: attempting to save JSON data over an existing
    /// SQLite database file must fail gracefully and not clobber the original.
    /// This direction was not checked at the storage layer before - it relied on
    /// distinct filenames in configuration to remain unreachable. This test pins
    /// the protection added to `JsonStorage::save`.
    #[test]
    fn test_json_rejects_sqlite_populated_file() {
        let temp_file = NamedTempFile::new().unwrap();

        // Populate the file with real data through the SQLite backend.
        let sqlite_storage = create_storage(StorageType::Sqlite, temp_file.path()).unwrap();
        let mut data = StorageData::new();
        let mut category = Category::new("Work".to_string(), None).unwrap();
        category.id = 1;
        data.categories.push(category.clone());
        let mut task =
            Task::new("Test task".to_string(), category.id, None, Priority::Medium).unwrap();
        task.id = 1;
        data.tasks.push(task);
        sqlite_storage.save(&data).unwrap();

        // Read the file to verify it has SQLite magic bytes.
        let file_contents = std::fs::read(temp_file.path()).unwrap();
        assert!(
            file_contents.len() >= 16 && &file_contents[..16] == b"SQLite format 3\0",
            "expected SQLite magic bytes in file"
        );

        // Pointing JSON storage at a file that already holds SQLite data
        // must be rejected gracefully with a typed StorageError - never a
        // panic, and never a silent success that silently overwrites the
        // SQLite database.
        let json_storage = create_storage(StorageType::Json, temp_file.path()).unwrap();
        let new_data = StorageData::new();
        let result = json_storage.save(&new_data);

        assert!(
            result.is_err(),
            "expected a graceful StorageError when saving JSON to a SQLite-populated file, got Ok(..)"
        );

        // Verify the file still contains SQLite data (not clobbered).
        let file_after = std::fs::read(temp_file.path()).unwrap();
        assert_eq!(
            file_contents, file_after,
            "SQLite file should not be modified when JSON save is rejected"
        );
    }

    /// Regression test: deleting a *category* reassigns its tasks
    /// to the magic "Uncategorized" ID (`category_manager::UNCATEGORIZED_ID`,
    /// which is 0). Before this fix, `get_deleted_tasks` treated
    /// `category_id == 0` as "this task is deleted", so a task merely
    /// orphaned by deleting its category would be misreported as deleted -
    /// and a subsequent `purge_deleted_tasks` flush would destroy it, even
    /// though the user never asked to delete that task. That is the
    /// data-loss scenario soft deletion exists to close off.
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
            "task orphaned by category deletion must not appear in get_deleted_tasks"
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
        // Seeded the way `CategoryManager` does it - load, push, save - rather
        // than through a storage-level `add_category` helper, so this test
        // exercises the same write path production uses.
        let mut data = storage.load().unwrap();
        data.categories.push(category);
        storage.save(&data).unwrap();

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
        // `get_tasks_by_title` is what task resolution matches names against,
        // and `get_all_tasks` is what `list` and its `--search` filter read:
        // between them, a soft-deleted task is unreachable by name and absent
        // from every listing.
        assert!(storage
            .get_tasks_by_title("Finish report")
            .unwrap()
            .is_empty());
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
    /// that decoupling fixed a second, subtler bug (editing a
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
