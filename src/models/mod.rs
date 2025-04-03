use crate::config::Config;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Task {
    pub id: u64,
    pub title: String,
    pub category_id: u64,
    pub completed: bool,
    pub priority: Priority,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Category {
    pub id: u64,
    pub name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum Priority {
    High,
    Medium,
    Low,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StorageData {
    pub tasks: Vec<Task>,
    pub categories: Vec<Category>,
    pub config: Config,
}
