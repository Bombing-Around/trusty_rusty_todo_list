use crate::config::Config;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub description: Option<String>,
    // 0 is the magic "Uncategorized" category ID - see
    // `category_manager::UNCATEGORIZED_ID`. It does NOT mean deleted; deletion
    // is tracked independently via `deleted_at` below.
    pub category_id: u64,
    pub completed: bool,
    pub priority: Priority,
    pub due_date: Option<DateTime<Utc>>,
    pub order: u32, // For custom sorting within category
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// When this task was soft-deleted, or `None` if it is live.
    ///
    /// `#[serde(default)]` keeps on-disk data written before this field
    /// existed loadable - it simply comes back as "not deleted", mirroring
    /// `StorageData::current_category`.
    #[serde(default)]
    pub deleted_at: Option<DateTime<Utc>>,
}

#[allow(dead_code)]
impl Task {
    pub fn new(
        title: String,
        category_id: u64,
        description: Option<String>,
        priority: Priority,
    ) -> Result<Self, TaskError> {
        if title.trim().is_empty() {
            return Err(TaskError::EmptyTitle);
        }

        Ok(Self {
            id: 0, // Will be set by storage layer
            title,
            description,
            category_id,
            completed: false,
            priority,
            due_date: None,
            order: 0,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            deleted_at: None,
        })
    }

    pub fn is_uncategorized(&self) -> bool {
        self.category_id == 0
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted_at.is_some()
    }

    /// Soft-deletes the task: it is hidden from listings/searches and becomes
    /// eligible for purging (see `Storage::purge_deleted_tasks`), but its
    /// `category_id` is left untouched. Keeping the real category is the
    /// whole point of this design - it's what makes `restore`
    /// trivial and stops category deletion from ever masquerading as task
    /// deletion.
    pub fn soft_delete(&mut self) {
        self.deleted_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Un-deletes the task. Because `soft_delete` never touched
    /// `category_id`, this is all a restore takes - the task goes straight
    /// back where it came from. Reachable from the CLI as
    /// `trtodo deleted restore <title or id>`.
    pub fn restore(&mut self) {
        self.deleted_at = None;
        self.updated_at = Utc::now();
    }

    /// The deletion timestamp rendered for humans: the "deleted" column of
    /// `trtodo deleted list`, the flush preview, and the restore
    /// disambiguation prompt all share this one format so the same task
    /// looks the same wherever it is shown.
    ///
    /// Timestamps are stored in UTC (`Utc::now`), and this deliberately
    /// prints them as such rather than converting to local time: nothing
    /// else in the CLI displays a timestamp yet, so there is no local-time
    /// convention to match, and labelling the zone is better than quietly
    /// implying one.
    ///
    /// A live task has no deletion timestamp; every caller of this today
    /// works from `Storage::get_deleted_tasks`, so the `None` arm exists
    /// only so this can never panic on a task that slipped through.
    pub fn deleted_at_display(&self) -> String {
        match self.deleted_at {
            Some(deleted_at) => deleted_at.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            None => "not deleted".to_string(),
        }
    }

    pub fn mark_completed(&mut self) {
        self.completed = true;
        self.updated_at = Utc::now();
    }

    pub fn mark_incomplete(&mut self) {
        self.completed = false;
        self.updated_at = Utc::now();
    }

    pub fn update_title(&mut self, new_title: String) -> Result<(), TaskError> {
        if new_title.trim().is_empty() {
            return Err(TaskError::EmptyTitle);
        }
        self.title = new_title;
        self.updated_at = Utc::now();
        Ok(())
    }

    pub fn move_to_category(&mut self, new_category_id: u64) {
        self.category_id = new_category_id;
        self.updated_at = Utc::now();
    }

    pub fn set_due_date(&mut self, due_date: Option<DateTime<Utc>>) {
        self.due_date = due_date;
        self.updated_at = Utc::now();
    }

    pub fn set_priority(&mut self, priority: Priority) {
        self.priority = priority;
        self.updated_at = Utc::now();
    }

    pub fn set_order(&mut self, order: u32) {
        self.order = order;
        self.updated_at = Utc::now();
    }
}

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum TaskError {
    #[error("Task title cannot be empty")]
    EmptyTitle,
    #[error("Invalid category ID: {0}")]
    InvalidCategory(u64),
    #[error("Invalid due date: {0}")]
    InvalidDueDate(String),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Category {
    pub id: u64,
    pub name: String,
    pub description: Option<String>,
    pub order: u32, // For custom sorting
    pub created_at: DateTime<Utc>,
}

#[allow(dead_code)]
impl Category {
    pub fn new(name: String, description: Option<String>) -> Result<Self, CategoryError> {
        if name.trim().is_empty() {
            return Err(CategoryError::EmptyName);
        }

        Ok(Self {
            id: 0, // Will be set by storage layer
            name,
            description,
            order: 0,
            created_at: Utc::now(),
        })
    }

    pub fn update_name(&mut self, new_name: String) -> Result<(), CategoryError> {
        if new_name.trim().is_empty() {
            return Err(CategoryError::EmptyName);
        }
        self.name = new_name;
        Ok(())
    }

    pub fn set_order(&mut self, order: u32) {
        self.order = order;
    }
}

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum CategoryError {
    #[error("Category name cannot be empty")]
    EmptyName,
    #[error("Category name already exists: {0}")]
    DuplicateName(String),
    #[error("Category not found: {0}")]
    NotFound(String),
    #[error(transparent)]
    Storage(#[from] StorageError),
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy)]
pub enum Priority {
    High,
    Medium,
    Low,
}

#[allow(dead_code)]
impl Priority {
    pub fn from_str(s: &str) -> Result<Self, PriorityError> {
        match s.to_lowercase().as_str() {
            "high" => Ok(Priority::High),
            "medium" => Ok(Priority::Medium),
            "low" => Ok(Priority::Low),
            _ => Err(PriorityError::InvalidPriority(s.to_string())),
        }
    }

    pub fn to_str(self) -> &'static str {
        match self {
            Priority::High => "high",
            Priority::Medium => "medium",
            Priority::Low => "low",
        }
    }

    pub fn default() -> Self {
        Priority::Medium
    }
}

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum PriorityError {
    #[error("Invalid priority value: {0}")]
    InvalidPriority(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StorageData {
    pub version: u32, // Schema version for future migrations
    pub tasks: Vec<Task>,
    pub categories: Vec<Category>,
    pub config: Config,
    /// The category context selected via `category use`, persisted between runs.
    ///
    /// `#[serde(default)]` keeps on-disk data written before this field existed
    /// loadable - it simply comes back as "no category selected".
    #[serde(default)]
    pub current_category: Option<u64>,
    pub last_sync: DateTime<Utc>,
}

impl StorageData {
    pub fn new() -> Self {
        Self {
            version: 1,
            tasks: Vec::new(),
            categories: Vec::new(),
            // The `config` field embedded in the task-data file is
            // vestigial - the real config lives in `trtodo-config.json` via
            // `ConfigStorage`. `Config::unset()` (nothing stored) is the
            // honest value here; resolving it to the documented defaults
            // would make this file start carrying a populated config blob
            // it was never meant to own.
            config: Config::unset(),
            current_category: None,
            last_sync: Utc::now(),
        }
    }

    pub fn validate(&self) -> Result<(), StorageError> {
        // Validate task references
        for task in &self.tasks {
            if task.category_id != 0 && !self.categories.iter().any(|c| c.id == task.category_id) {
                return Err(StorageError::InvalidTaskCategory(task.id, task.category_id));
            }
        }

        // Validate category uniqueness
        let mut names = std::collections::HashSet::new();
        for category in &self.categories {
            if !names.insert(category.name.to_lowercase()) {
                return Err(StorageError::DuplicateCategory(category.name.clone()));
            }
        }

        Ok(())
    }
}

#[allow(dead_code)]
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Invalid data: {0}")]
    InvalidData(String),
    #[error("Model error: {0}")]
    Model(String),
    #[error("Invalid task category: task {0} references non-existent category {1}")]
    InvalidTaskCategory(u64, u64),
    #[error("Duplicate category name: {0}")]
    DuplicateCategory(String),
    #[error("File format mismatch: {0}")]
    FormatMismatch(String),
}
