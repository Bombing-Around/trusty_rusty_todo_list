use crate::config::Config;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub description: Option<String>,
    pub category_id: u64, // 0 for uncategorized
    pub completed: bool,
    pub priority: Priority,
    pub due_date: Option<DateTime<Utc>>,
    pub order: u32, // For custom sorting within category
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
        })
    }

    pub fn is_uncategorized(&self) -> bool {
        self.category_id == 0
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
    pub last_sync: DateTime<Utc>,
}

impl StorageData {
    pub fn new() -> Self {
        Self {
            version: 1,
            tasks: Vec::new(),
            categories: Vec::new(),
            config: Config::default(),
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
}
