//! Task management.
//!
//! `TaskManager` mirrors `CategoryManager`: it sits between the CLI and the
//! `Storage` trait and owns the task behaviors from the README - add,
//! (soft) delete, update, check/uncheck, moving tasks between categories,
//! filtered listing, and the "match by name, prompt if ambiguous" rule (see
//! `crate::prompter`).
//!
//! It also owns the *deleted* side of that world (`trt deleted ...`):
//! listing what is soft-deleted, resolving a reference among only those
//! tasks, restoring one, and flushing them all. Those live here rather than
//! in a module of their own because they are the same tasks under a
//! different scope - `resolve_deleted_task` is `resolve_task` with
//! `live_tasks()` swapped for `get_deleted_tasks()`, and shares its
//! disambiguation step verbatim.

use crate::category_manager::UNCATEGORIZED_ID;
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
            _ => Self::disambiguate(reference, candidates, prompter, |t| {
                format!("#{} - {} (category {})", t.id, t.title, t.category_id)
            }),
        }
    }

    /// The shared tail of `resolve_task` and `resolve_deleted_task`: several
    /// tasks answer to the same reference, so the README's "prompt the user
    /// for which item on which to operate" rule fires.
    ///
    /// `label` is a parameter rather than a fixed format because the two
    /// callers have different things worth showing: a live match is told
    /// apart by its category, while two *deleted* tasks with the same title
    /// in the same category are told apart only by when each was deleted.
    fn disambiguate(
        reference: &str,
        candidates: Vec<Task>,
        prompter: &mut dyn Prompter,
        label: impl Fn(&Task) -> String,
    ) -> Result<Task, TaskManagerError> {
        let labels: Vec<String> = candidates.iter().map(label).collect();
        let message = format!("Multiple tasks named '{reference}' were found; which one?");
        let index = prompter.choose(&message, &labels)?;
        Ok(candidates
            .into_iter()
            .nth(index)
            .expect("prompter must return an index within range"))
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

    /// Soft-deletes a task: it disappears from every listing/
    /// search but keeps its real category and can still be found by ID.
    pub fn delete_task(&self, task_id: u64) -> Result<(), TaskManagerError> {
        Ok(self.storage.soft_delete_task(task_id)?)
    }

    /// Every currently soft-deleted task, oldest deletion first
    /// (`trt deleted list`).
    ///
    /// Sorted by `deleted_at` deliberately: this list doubles as the preview
    /// of what a `flush` would destroy and of what the automatic
    /// `deleted-task-lifespan` sweep will reach first, and both of those act
    /// on the oldest deletions - so the tasks nearest destruction are the
    /// ones at the top. `id` breaks ties, since two tasks deleted in the
    /// same second are otherwise ordered arbitrarily.
    pub fn list_deleted(&self) -> Result<Vec<Task>, TaskManagerError> {
        let mut tasks = self.storage.get_deleted_tasks()?;
        tasks.sort_by(|a, b| a.deleted_at.cmp(&b.deleted_at).then(a.id.cmp(&b.id)));
        Ok(tasks)
    }

    /// Resolves a `<title or id>` CLI argument to a single *soft-deleted*
    /// task, for `trt deleted restore`.
    ///
    /// This is the inverse scope of `resolve_task`, and it has to be its own
    /// method rather than a flag on that one: every title/ID lookup helper
    /// on `Storage` routes through `live_tasks()` and so can never see a
    /// deleted task, while `resolve_task` goes further and explicitly
    /// rejects a by-ID hit whose `deleted_at` is set. Bending either to also
    /// mean "but sometimes only the deleted ones" would put a
    /// restore-shaped special case inside the path every ordinary task
    /// command takes. Searching `get_deleted_tasks()` here keeps that risk
    /// where it belongs: it is impossible for this method to return a live
    /// task, and impossible for `resolve_task` to return a deleted one.
    ///
    /// There is no `category_id` scope parameter, and `deleted restore` has
    /// no `--category`: a soft-deleted task is invisible to `list`, so the
    /// user has no way to know which category the thing they want back was
    /// in without running `deleted list` first - which prints the ID, and an
    /// ID needs no disambiguation. An ambiguous *title* still prompts, via
    /// the same `Prompter` seam every other task command uses.
    pub fn resolve_deleted_task(
        &self,
        reference: &str,
        prompter: &mut dyn Prompter,
    ) -> Result<Task, TaskManagerError> {
        let deleted = self.list_deleted()?;
        let wanted = reference.to_lowercase();
        // Case-insensitive, matching `Storage::get_tasks_by_title`.
        let candidates: Vec<Task> = deleted
            .iter()
            .filter(|t| t.title.to_lowercase() == wanted)
            .cloned()
            .collect();

        match candidates.len() {
            0 => reference
                .parse::<u64>()
                .ok()
                .and_then(|id| deleted.into_iter().find(|t| t.id == id))
                .ok_or_else(|| TaskManagerError::NotFound(reference.to_string())),
            1 => Ok(candidates.into_iter().next().expect("len checked above")),
            _ => Self::disambiguate(reference, candidates, prompter, |t| {
                format!(
                    "#{} - {} (category {}, deleted {})",
                    t.id,
                    t.title,
                    t.category_id,
                    t.deleted_at_display()
                )
            }),
        }
    }

    /// Restores a soft-deleted task, returning the category ID it
    /// actually landed in so the CLI can name it in its confirmation.
    ///
    /// Normally that is simply the task's own `category_id`, untouched since
    /// before the delete - which is the entire point of soft deletion keeping
    /// the real `category_id`.
    /// See `restore_destination` for the one case where it isn't.
    pub fn restore_task(&self, mut task: Task) -> Result<u64, TaskManagerError> {
        let destination =
            restore_destination(task.category_id, self.category_exists(task.category_id)?);
        task.restore();
        if destination != task.category_id {
            task.move_to_category(destination);
        }
        self.storage.update_task(task)?;
        Ok(destination)
    }

    /// Whether a task's `category_id` still refers to something real.
    /// `UNCATEGORIZED_ID` always does: it is synthesized rather than stored
    /// (see `CategoryManager::get_category`), so a plain storage lookup
    /// would wrongly report it missing.
    fn category_exists(&self, category_id: u64) -> Result<bool, StorageError> {
        if category_id == UNCATEGORIZED_ID {
            return Ok(true);
        }
        Ok(self.storage.get_category(category_id)?.is_some())
    }

    /// Permanently removes every currently soft-deleted task, unconditionally
    /// (`trt deleted flush`). See `Storage::purge_all_deleted_tasks` for
    /// why this is a distinct primitive from the automatic,
    /// `deleted-task-lifespan`-gated purge below.
    ///
    /// Returns the tasks it removed, not just a count: the CLI
    /// reports *what* was destroyed, and by the time it could go and look
    /// the rows are gone. The snapshot is taken here, immediately before the
    /// purge, so what comes back is what this call actually destroyed -
    /// never a stale list assembled somewhere further up.
    pub fn flush_deleted(&self) -> Result<Vec<Task>, TaskManagerError> {
        let flushed = self.list_deleted()?;
        self.storage.purge_all_deleted_tasks()?;
        Ok(flushed)
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

/// Where a restored task should land, given the category it remembers and
/// whether that category still exists. Worth a deliberate decision rather
/// than an accident, since both outcomes are defensible.
///
/// The decision: fall back to Uncategorized. A task whose category is gone
/// still has to go *somewhere*, Uncategorized is the one category that can
/// never be deleted (it is synthesized, not stored), and it is already where
/// `CategoryManager::delete_category` sends orphaned tasks - so a restore
/// lands the task exactly where it would have been had it been live all
/// along. Refusing the restore instead would leave the user's only copy of
/// the task in the one place a `flush` destroys.
///
/// In practice this is a backstop, not a path users can reach:
/// `delete_category` rewrites the `category_id` of *every* task in the
/// category it removes, soft-deleted ones included, and `StorageData::validate`
/// rejects a dangling task->category reference on both save and load. It is
/// implemented anyway because the alternative - a restore that silently
/// resurrects a task into a category that isn't there - is a data-integrity
/// error that would only surface much later.
fn restore_destination(category_id: u64, category_exists: bool) -> u64 {
    if category_exists {
        category_id
    } else {
        UNCATEGORIZED_ID
    }
}

/// Asks the user to confirm an irreversible `deleted flush` of `pending`
/// tasks, through the same `Prompter` seam as every other prompt in the
/// app. Returns whether they said yes.
///
/// A `PromptError::NotInteractive` here is not a failure of this function -
/// it is the answer "there is nobody to ask", which the CLI turns into a
/// refusal to flush. See `main::run_deleted_command`.
///
/// "No" is offered first, so the reflexive `1` at an unexpected prompt keeps
/// the tasks rather than destroys them. `Prompter::choose` is reused as-is
/// rather than growing a `confirm` method: a yes/no question is a two-option
/// choice, and reusing it means `StdinPrompter`'s non-interactive detection,
/// input parsing, and range checking all apply here for free.
pub fn confirm_flush(prompter: &mut dyn Prompter, pending: usize) -> Result<bool, PromptError> {
    let message =
        format!("Permanently remove {pending} soft-deleted task(s)? This cannot be undone.");
    let options = vec![
        "No, keep them".to_string(),
        "Yes, permanently remove them".to_string(),
    ];
    Ok(prompter.choose(&message, &options)? == 1)
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
    fn resolve_task_prompts_across_categories_when_unscoped() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        add_categories(storage, &["Work", "Home"]);
        let _work_id = add(storage, "Buy milk", 1, Priority::Medium);
        let home_id = add(storage, "Buy milk", 2, Priority::Medium);

        // No category scope at all (`None`): this is the README's
        // cross-category disambiguation - the same title exists in two
        // different categories, so the prompter is consulted even though
        // neither `--category` nor a context narrowed the search.
        let mut prompter = ScriptedPrompter::new(vec![Ok(1)]);
        let resolved = manager
            .resolve_task("Buy milk", None, &mut prompter)
            .unwrap();
        assert_eq!(resolved.id, home_id);
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

        // The flush reports the tasks it destroyed, not just how many
        // - nobody can go and look afterwards.
        let purged = manager.flush_deleted().unwrap();
        assert_eq!(purged.len(), 1);
        assert_eq!(purged[0].id, deleted_id);
        assert_eq!(purged[0].title, "Buy milk");

        assert!(storage.get_task(deleted_id).unwrap().is_none());
        assert!(storage.get_task(live_id).unwrap().is_some());

        // Nothing left to flush the second time.
        assert!(manager.flush_deleted().unwrap().is_empty());
    }

    /// Backdates an existing task's deletion timestamp, so tests can tell
    /// apart deletions that would otherwise land in the same instant.
    fn backdate_deletion(storage: &dyn Storage, task_id: u64, days: i64) {
        let mut task = storage.get_task(task_id).unwrap().unwrap();
        task.deleted_at = Some(chrono::Utc::now() - chrono::Duration::days(days));
        storage.update_task(task).unwrap();
    }

    #[test]
    fn list_deleted_returns_only_deleted_tasks_oldest_first() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        let recent = add(storage, "Buy milk", 0, Priority::Medium);
        let ancient = add(storage, "Walk dog", 0, Priority::Medium);
        let live = add(storage, "Still here", 0, Priority::Medium);
        manager.delete_task(recent).unwrap();
        manager.delete_task(ancient).unwrap();
        backdate_deletion(storage, ancient, 30);

        let listed = manager.list_deleted().unwrap();
        assert_eq!(listed.len(), 2);
        // Oldest deletion first: those are the ones a flush - and the
        // age-gated automatic purge - reaches first.
        assert_eq!(listed[0].id, ancient);
        assert_eq!(listed[1].id, recent);
        assert!(!listed.iter().any(|t| t.id == live));
    }

    #[test]
    fn list_deleted_is_empty_when_nothing_has_been_deleted() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        add(storage, "Buy milk", 0, Priority::Medium);

        assert!(manager.list_deleted().unwrap().is_empty());
    }

    #[test]
    fn resolve_deleted_task_matches_by_title_then_by_id() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        let id = add(storage, "Buy milk", 0, Priority::Medium);
        manager.delete_task(id).unwrap();

        let mut prompter = ScriptedPrompter::new(vec![]);
        let by_name = manager
            .resolve_deleted_task("Buy milk", &mut prompter)
            .unwrap();
        assert_eq!(by_name.id, id);

        let by_id = manager
            .resolve_deleted_task(&id.to_string(), &mut prompter)
            .unwrap();
        assert_eq!(by_id.id, id);
    }

    #[test]
    fn resolve_deleted_task_never_matches_a_live_task() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        let live = add(storage, "Buy milk", 0, Priority::Medium);
        let mut prompter = ScriptedPrompter::new(vec![]);

        // This is the inverse of `resolve_task`'s scope: a live task is not
        // restorable, by either of its handles.
        assert!(matches!(
            manager
                .resolve_deleted_task("Buy milk", &mut prompter)
                .unwrap_err(),
            TaskManagerError::NotFound(_)
        ));
        assert!(matches!(
            manager
                .resolve_deleted_task(&live.to_string(), &mut prompter)
                .unwrap_err(),
            TaskManagerError::NotFound(_)
        ));
    }

    #[test]
    fn resolve_deleted_task_ignores_a_live_namesake_of_a_deleted_task() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        add_categories(storage, &["Work", "Home"]);
        let deleted = add(storage, "Buy milk", 1, Priority::Medium);
        let _live_namesake = add(storage, "Buy milk", 2, Priority::Medium);
        manager.delete_task(deleted).unwrap();

        // Only one of the two is deleted, so this is unambiguous and must
        // not prompt - the live namesake is simply not a candidate.
        let mut prompter = ScriptedPrompter::new(vec![]);
        let resolved = manager
            .resolve_deleted_task("Buy milk", &mut prompter)
            .unwrap();
        assert_eq!(resolved.id, deleted);
    }

    #[test]
    fn resolve_deleted_task_prompts_when_the_title_is_ambiguous() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        add_categories(storage, &["Work", "Home"]);
        let work = add(storage, "Buy milk", 1, Priority::Medium);
        let home = add(storage, "Buy milk", 2, Priority::Medium);
        manager.delete_task(work).unwrap();
        manager.delete_task(home).unwrap();
        backdate_deletion(storage, work, 5);

        // Same README rule as every other task command, over the deleted
        // scope. `list_deleted`'s ordering decides what index 1 means: the
        // backdated `work` sorts first, so the scripted answer picks `home`.
        let mut prompter = ScriptedPrompter::new(vec![Ok(1)]);
        let resolved = manager
            .resolve_deleted_task("Buy milk", &mut prompter)
            .unwrap();
        assert_eq!(resolved.id, home);
    }

    #[test]
    fn resolve_deleted_task_propagates_non_interactive_prompt_error() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        let first = add(storage, "Buy milk", 0, Priority::Medium);
        let second = add(storage, "Buy milk", 0, Priority::Low);
        manager.delete_task(first).unwrap();
        manager.delete_task(second).unwrap();

        let mut prompter = ScriptedPrompter::new(vec![Err(PromptError::NotInteractive)]);
        let err = manager
            .resolve_deleted_task("Buy milk", &mut prompter)
            .unwrap_err();
        assert!(matches!(
            err,
            TaskManagerError::Prompt(PromptError::NotInteractive)
        ));
    }

    #[test]
    fn resolve_deleted_task_not_found_is_a_clean_error() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        let mut prompter = ScriptedPrompter::new(vec![]);

        let err = manager
            .resolve_deleted_task("Nope", &mut prompter)
            .unwrap_err();
        assert!(matches!(err, TaskManagerError::NotFound(_)));
    }

    #[test]
    fn restore_task_returns_it_to_its_original_category_and_to_listings() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        add_categories(storage, &["Work"]);
        let id = add(storage, "Buy milk", 1, Priority::High);
        manager.delete_task(id).unwrap();
        assert!(manager.list_tasks(None, false, None).unwrap().is_empty());

        let mut prompter = ScriptedPrompter::new(vec![]);
        let task = manager
            .resolve_deleted_task("Buy milk", &mut prompter)
            .unwrap();
        let destination = manager.restore_task(task).unwrap();

        // Lossless: same category it was deleted from (keeping the real
        // `category_id` is what makes this free), and the priority survives
        // too.
        assert_eq!(destination, 1);
        let restored = storage.get_task(id).unwrap().unwrap();
        assert!(!restored.is_deleted());
        assert_eq!(restored.category_id, 1);
        assert_eq!(restored.priority, Priority::High);

        // Visible to `list` again, and gone from the deleted set - so a
        // later flush can no longer destroy it.
        let listed = manager.list_tasks(None, false, None).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);
        assert!(manager.list_deleted().unwrap().is_empty());
        assert!(manager.flush_deleted().unwrap().is_empty());
    }

    #[test]
    fn restore_task_leaves_an_uncategorized_task_uncategorized() {
        let test_storage = TestStorage::new();
        let storage = test_storage.storage();
        let manager = TaskManager::new(storage);
        let id = add(storage, "Buy milk", UNCATEGORIZED_ID, Priority::Medium);
        manager.delete_task(id).unwrap();

        // Uncategorized is synthesized rather than stored, so a naive
        // "does this category exist?" check would wrongly treat it as gone.
        let task = storage.get_task(id).unwrap().unwrap();
        assert_eq!(manager.restore_task(task).unwrap(), UNCATEGORIZED_ID);
    }

    #[test]
    fn restore_destination_falls_back_to_uncategorized_when_the_category_is_gone() {
        // The category still exists: the task goes back exactly where it was.
        assert_eq!(restore_destination(7, true), 7);
        // It doesn't: Uncategorized, the one category that cannot be deleted,
        // rather than a dangling reference or a refused restore.
        assert_eq!(restore_destination(7, false), UNCATEGORIZED_ID);
    }

    #[test]
    fn confirm_flush_requires_an_explicit_yes() {
        // Option 1 ("No, keep them") is offered first on purpose, so a
        // reflexive `1` at an unexpected prompt keeps the tasks.
        let mut declined = ScriptedPrompter::new(vec![Ok(0)]);
        assert_eq!(confirm_flush(&mut declined, 3), Ok(false));

        let mut confirmed = ScriptedPrompter::new(vec![Ok(1)]);
        assert_eq!(confirm_flush(&mut confirmed, 3), Ok(true));
    }

    #[test]
    fn confirm_flush_reports_a_non_interactive_terminal_rather_than_assuming_yes() {
        // The CLI turns this into a refusal to flush (`CliError::FlushNotConfirmed`);
        // what matters here is that "nobody to ask" is never silently "yes".
        let mut prompter = ScriptedPrompter::new(vec![Err(PromptError::NotInteractive)]);
        assert_eq!(
            confirm_flush(&mut prompter, 1),
            Err(PromptError::NotInteractive)
        );
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
