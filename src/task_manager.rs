//! Task management.
//!
//! `TaskManager` mirrors `CategoryManager`: it sits between the CLI and the
//! `Storage` trait and owns the task behaviors from the README - add,
//! (soft) delete, update, check/uncheck, moving tasks between categories,
//! filtered listing, and the "match by name, prompt if ambiguous" rule (see
//! `crate::prompter`).

use crate::models::{Priority, StorageError, Task, TaskError};
use crate::prompter::{PromptError, Prompter};
use crate::storage::Storage;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TaskManagerError {
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Task(#[from] TaskError),
    #[error(transparent)]
    Prompt(#[from] PromptError),
    #[error("no task matching '{0}' was found")]
    NotFound(String),
}

pub struct TaskManager<'a> {
    storage: &'a dyn Storage,
}

impl<'a> TaskManager<'a> {
    pub fn new(storage: &'a dyn Storage) -> Self {
        Self { storage }
    }

    /// Adds a new task, handing out the next task ID the same way
    /// `CategoryManager::add_category` hands out category IDs.
    pub fn add_task(
        &self,
        title: String,
        category_id: u64,
        priority: Priority,
        description: Option<String>,
    ) -> Result<u64, TaskManagerError> {
        let mut task = Task::new(title, category_id, description, priority)?;
        task.id = self.storage.get_next_task_id()?;
        let id = task.id;
        self.storage.add_task(task)?;
        Ok(id)
    }

    /// Resolves a `<title or id>` CLI argument to a single live task,
    /// following the README's "name, then numeric ID" convention. When
    /// `category_id` is `Some`, both the name and ID lookups are scoped to
    /// that category - this is what lets `--category` (or the current
    /// category context) disambiguate two same-named tasks in different
    /// categories without ever prompting.
    ///
    /// If more than one *live* task within scope shares the given title, the
    /// README's "prompt the user for which item" rule kicks in via
    /// `prompter` - see `crate::prompter` for why that is a trait rather
    /// than an inline `stdin` call.
    pub fn resolve_task(
        &self,
        reference: &str,
        category_id: Option<u64>,
        prompter: &mut dyn Prompter,
    ) -> Result<Task, TaskManagerError> {
        let candidates = self.tasks_by_title_in_scope(reference, category_id)?;

        match candidates.len() {
            0 => self
                .task_by_id_in_scope(reference, category_id)?
                .ok_or_else(|| TaskManagerError::NotFound(reference.to_string())),
            1 => Ok(candidates.into_iter().next().expect("len checked above")),
            _ => {
                let labels: Vec<String> = candidates
                    .iter()
                    .map(|t| format!("#{} - {} (category {})", t.id, t.title, t.category_id))
                    .collect();
                let message = format!("Multiple tasks named '{reference}' were found; which one?");
                let index = prompter.choose(&message, &labels)?;
                Ok(candidates
                    .into_iter()
                    .nth(index)
                    .expect("prompter must return an index within range"))
            }
        }
    }

    fn tasks_by_title_in_scope(
        &self,
        title: &str,
        category_id: Option<u64>,
    ) -> Result<Vec<Task>, StorageError> {
        let matches = self.storage.get_tasks_by_title(title)?;
        Ok(match category_id {
            Some(id) => matches
                .into_iter()
                .filter(|t| t.category_id == id)
                .collect(),
            None => matches,
        })
    }

    /// Falls back to a numeric-ID lookup. Deliberately re-checks
    /// `deleted_at`/`category_id` itself rather than trusting
    /// `Storage::get_task` alone: that method intentionally does *not*
    /// filter out soft-deleted tasks (restore/purge need to reach them), so
    /// a deleted task's ID must not resurface as a live match here.
    fn task_by_id_in_scope(
        &self,
        reference: &str,
        category_id: Option<u64>,
    ) -> Result<Option<Task>, StorageError> {
        let Ok(id) = reference.parse::<u64>() else {
            return Ok(None);
        };
        let Some(task) = self.storage.get_task(id)? else {
            return Ok(None);
        };
        if task.deleted_at.is_some() {
            return Ok(None);
        }
        if let Some(scope) = category_id {
            if task.category_id != scope {
                return Ok(None);
            }
        }
        Ok(Some(task))
    }

    /// Soft-deletes a task (issue #29): it disappears from every listing/
    /// search but keeps its real category and can still be found by ID.
    pub fn delete_task(&self, task_id: u64) -> Result<(), TaskManagerError> {
        Ok(self.storage.soft_delete_task(task_id)?)
    }

    /// Permanently removes every currently soft-deleted task, unconditionally
    /// (`trtodo deleted flush`). Returns how many were removed, for the CLI
    /// to report back to the user. See `Storage::purge_all_deleted_tasks`
    /// for why this is a distinct primitive from the automatic,
    /// `deleted-task-lifespan`-gated purge below.
    pub fn flush_deleted(&self) -> Result<usize, TaskManagerError> {
        Ok(self.storage.purge_all_deleted_tasks()?)
    }

    /// Automatically purges tasks that were soft-deleted more than
    /// `days_threshold` days ago (README: `deleted-task-lifespan`). A
    /// threshold of `0` means "never" and is handled by
    /// `Storage::purge_deleted_tasks` itself. Distinct from `flush_deleted`:
    /// this is the age-gated sweep run automatically on every invocation
    /// (see `main::open_storage`), not the manual, unconditional flush.
    pub fn purge_expired_deleted(&self, days_threshold: u32) -> Result<(), TaskManagerError> {
        Ok(self.storage.purge_deleted_tasks(days_threshold)?)
    }

    pub fn rename_task(&self, mut task: Task, new_title: String) -> Result<(), TaskManagerError> {
        task.update_title(new_title)?;
        self.storage.update_task(task)?;
        Ok(())
    }

    pub fn set_completed(&self, mut task: Task, completed: bool) -> Result<(), TaskManagerError> {
        if completed {
            task.mark_completed();
        } else {
            task.mark_incomplete();
        }
        self.storage.update_task(task)?;
        Ok(())
    }

    /// Checks or unchecks every live task in `category_id`, returning how
    /// many were touched.
    pub fn set_all_completed(
        &self,
        category_id: u64,
        completed: bool,
    ) -> Result<usize, TaskManagerError> {
        let tasks = self.storage.get_tasks_by_category(category_id)?;
        let count = tasks.len();
        for mut task in tasks {
            if completed {
                task.mark_completed();
            } else {
                task.mark_incomplete();
            }
            self.storage.update_task(task)?;
        }
        Ok(count)
    }

    pub fn move_task(&self, task_id: u64, new_category_id: u64) -> Result<(), TaskManagerError> {
        Ok(self
            .storage
            .move_task_to_category(task_id, new_category_id)?)
    }

    /// Lists live tasks matching the `list` command's filters.
    ///
    /// `completed_only` mirrors the CLI's `--completed` boolean flag: unset
    /// shows every live task regardless of completion state (the README's
    /// "list all tasks"); set narrows the result to just the completed ones.
    /// There's no dedicated flag for "incomplete only" in the CLI surface, so
    /// that combination isn't needed here.
    ///
    /// Reuses the storage layer's already-tested combinators for the
    /// (completion, priority) pair and only does its own filtering for
    /// `--search`, since no storage-level combinator covers search alongside
    /// the other two filters at once.
    pub fn list_tasks(
        &self,
        search: Option<&str>,
        completed_only: bool,
        priority: Option<Priority>,
    ) -> Result<Vec<Task>, TaskManagerError> {
        let mut tasks = match (completed_only, priority) {
            (true, Some(p)) => self.storage.get_tasks_by_completion_and_priority(true, p)?,
            (true, None) => self.storage.get_completed_tasks()?,
            (false, Some(p)) => self.storage.get_tasks_by_priority(p)?,
            (false, None) => self.storage.get_all_tasks()?,
        };

        if let Some(query) = search {
            let query = query.to_lowercase();
            tasks.retain(|t| t.title.to_lowercase().contains(&query));
        }

        Ok(tasks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::category_manager::CategoryManager;
    use crate::prompter::ScriptedPrompter;
    use crate::storage::test_utils::TestStorage;

    fn add(storage: &dyn Storage, title: &str, category_id: u64, priority: Priority) -> u64 {
        TaskManager::new(storage)
            .add_task(title.to_string(), category_id, priority, None)
            .unwrap()
    }

    /// `StorageData::validate` requires every non-zero `category_id` on a
    /// task to reference a real, existing category, so tests that put tasks
    /// in category 1/2 need those categories to actually exist first.
    fn add_categories(storage: &dyn Storage, names: &[&str]) {
        let mut manager = CategoryManager::new(storage);
        for name in names {
            manager.add_category(name.to_string(), None).unwrap();
        }
    }

    #[test]
    fn resolve_task_matches_by_title_then_by_id() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        let id = add(storage, "Buy milk", 0, Priority::Medium);

        let mut prompter = ScriptedPrompter::new(vec![]);
        let by_name = manager
            .resolve_task("Buy milk", None, &mut prompter)
            .unwrap();
        assert_eq!(by_name.id, id);

        let by_id = manager
            .resolve_task(&id.to_string(), None, &mut prompter)
            .unwrap();
        assert_eq!(by_id.id, id);
    }

    #[test]
    fn resolve_task_scopes_to_category_when_given() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        add_categories(storage, &["Work", "Home"]);
        let work_id = add(storage, "Buy milk", 1, Priority::Medium);
        let _home_id = add(storage, "Buy milk", 2, Priority::Medium);

        // Same title in two categories: scoping to one category resolves
        // cleanly, without ever consulting the prompter.
        let mut prompter = ScriptedPrompter::new(vec![]);
        let resolved = manager
            .resolve_task("Buy milk", Some(1), &mut prompter)
            .unwrap();
        assert_eq!(resolved.id, work_id);
    }

    #[test]
    fn resolve_task_prompts_when_multiple_matches_share_scope() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        add_categories(storage, &["Work"]);
        let _first = add(storage, "Buy milk", 1, Priority::Medium);
        let second = add(storage, "Buy milk", 1, Priority::High);

        // Two tasks named "Buy milk" in the *same* category: the prompter is
        // consulted, and its scripted answer picks which one comes back.
        let mut prompter = ScriptedPrompter::new(vec![Ok(1)]);
        let resolved = manager
            .resolve_task("Buy milk", Some(1), &mut prompter)
            .unwrap();
        assert_eq!(resolved.id, second);
    }

    #[test]
    fn resolve_task_propagates_non_interactive_prompt_error() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        add_categories(storage, &["Work"]);
        add(storage, "Buy milk", 1, Priority::Medium);
        add(storage, "Buy milk", 1, Priority::Low);

        // Simulates running with no terminal attached: the disambiguation
        // must surface as a clean, typed error rather than hang or panic.
        let mut prompter = ScriptedPrompter::new(vec![Err(PromptError::NotInteractive)]);
        let err = manager
            .resolve_task("Buy milk", Some(1), &mut prompter)
            .unwrap_err();
        assert!(matches!(
            err,
            TaskManagerError::Prompt(PromptError::NotInteractive)
        ));
    }

    #[test]
    fn resolve_task_not_found_is_a_clean_error() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        let mut prompter = ScriptedPrompter::new(vec![]);

        let err = manager
            .resolve_task("Nope", None, &mut prompter)
            .unwrap_err();
        assert!(matches!(err, TaskManagerError::NotFound(_)));
    }

    #[test]
    fn resolve_task_by_id_is_still_scoped_to_category() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        add_categories(storage, &["Work", "Home"]);
        let id = add(storage, "Buy milk", 1, Priority::Medium);
        let mut prompter = ScriptedPrompter::new(vec![]);

        // The task exists, but not in category 2 - looking it up by ID
        // scoped to the wrong category must not find it.
        let err = manager
            .resolve_task(&id.to_string(), Some(2), &mut prompter)
            .unwrap_err();
        assert!(matches!(err, TaskManagerError::NotFound(_)));
    }

    #[test]
    fn delete_task_is_soft_and_hides_from_listing_but_stays_reachable_by_id() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        let id = add(storage, "Buy milk", 0, Priority::Medium);

        manager.delete_task(id).unwrap();

        let listed = manager.list_tasks(None, false, None).unwrap();
        assert!(listed.is_empty());
        assert!(storage.get_task(id).unwrap().unwrap().is_deleted());
    }

    #[test]
    fn flush_deleted_purges_deleted_tasks_but_leaves_live_ones_alone() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        let deleted_id = add(storage, "Buy milk", 0, Priority::Medium);
        let live_id = add(storage, "Walk dog", 0, Priority::Medium);
        manager.delete_task(deleted_id).unwrap();

        let purged = manager.flush_deleted().unwrap();
        assert_eq!(purged, 1);

        assert!(storage.get_task(deleted_id).unwrap().is_none());
        assert!(storage.get_task(live_id).unwrap().is_some());

        // Nothing left to flush the second time.
        assert_eq!(manager.flush_deleted().unwrap(), 0);
    }

    #[test]
    fn purge_expired_deleted_respects_the_zero_means_never_default() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        let id = add(storage, "Ancient task", 0, Priority::Low);
        manager.delete_task(id).unwrap();

        // Backdate the deletion well past any plausible threshold.
        let mut task = storage.get_task(id).unwrap().unwrap();
        task.deleted_at = Some(chrono::Utc::now() - chrono::Duration::days(3650));
        storage.update_task(task).unwrap();

        manager.purge_expired_deleted(0).unwrap();
        assert!(storage.get_task(id).unwrap().is_some());

        manager.purge_expired_deleted(30).unwrap();
        assert!(storage.get_task(id).unwrap().is_none());
    }

    #[test]
    fn list_tasks_filters_by_completed_priority_and_search() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        let milk = add(storage, "Buy milk", 0, Priority::High);
        let bread = add(storage, "Buy bread", 0, Priority::Low);
        manager
            .set_completed(storage.get_task(milk).unwrap().unwrap(), true)
            .unwrap();

        let all = manager.list_tasks(None, false, None).unwrap();
        assert_eq!(all.len(), 2);

        let completed = manager.list_tasks(None, true, None).unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].id, milk);

        let high_priority = manager
            .list_tasks(None, false, Some(Priority::High))
            .unwrap();
        assert_eq!(high_priority.len(), 1);
        assert_eq!(high_priority[0].id, milk);

        let search = manager.list_tasks(Some("bread"), false, None).unwrap();
        assert_eq!(search.len(), 1);
        assert_eq!(search[0].id, bread);
    }

    #[test]
    fn set_all_completed_touches_only_the_given_category() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        add_categories(storage, &["Work", "Home"]);
        add(storage, "Work task", 1, Priority::Medium);
        add(storage, "Home task", 2, Priority::Medium);

        let touched = manager.set_all_completed(1, true).unwrap();
        assert_eq!(touched, 1);

        let work_tasks = storage.get_tasks_by_category(1).unwrap();
        assert!(work_tasks[0].completed);
        let home_tasks = storage.get_tasks_by_category(2).unwrap();
        assert!(!home_tasks[0].completed);
    }

    #[test]
    fn move_task_updates_category() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        add_categories(storage, &["Work", "Home"]);
        let id = add(storage, "Buy milk", 1, Priority::Medium);

        manager.move_task(id, 2).unwrap();

        let moved = storage.get_task(id).unwrap().unwrap();
        assert_eq!(moved.category_id, 2);
    }
}
