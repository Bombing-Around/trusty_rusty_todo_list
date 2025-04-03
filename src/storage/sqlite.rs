use super::Storage;
use super::StorageError;
use crate::models::{Category, Priority, StorageData, Task};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Mutex;

#[allow(dead_code)]
const SCHEMA_VERSION: i32 = 1;

#[allow(dead_code)]
const INIT_SCHEMA: &str = r#"
-- Create schema version table first
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
);

-- Create categories table
CREATE TABLE IF NOT EXISTS categories (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL
);

-- Create tasks table
CREATE TABLE IF NOT EXISTS tasks (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    category_id INTEGER NOT NULL,
    completed BOOLEAN NOT NULL DEFAULT 0,
    priority TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (category_id) REFERENCES categories(id)
);
"#;

#[allow(dead_code)]
pub struct SqliteStorage {
    conn: Mutex<Connection>,
}

#[allow(dead_code)]
impl SqliteStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let conn = Connection::open(path)
            .map_err(|e| StorageError::Storage(format!("Failed to open SQLite database: {}", e)))?;

        let storage = Self {
            conn: Mutex::new(conn),
        };
        storage.initialize_schema()?;
        Ok(storage)
    }

    fn initialize_schema(&self) -> Result<(), StorageError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| StorageError::Storage(format!("Failed to lock connection: {}", e)))?;

        // First create the tables
        conn.execute_batch(INIT_SCHEMA)
            .map_err(|e| StorageError::Storage(format!("Failed to initialize schema: {}", e)))?;

        // Then set the schema version
        conn.execute(
            "INSERT OR REPLACE INTO schema_version (version) VALUES (?1)",
            params![SCHEMA_VERSION],
        )
        .map_err(|e| StorageError::Storage(format!("Failed to set schema version: {}", e)))?;

        Ok(())
    }

    fn priority_to_string(priority: &Priority) -> &'static str {
        match priority {
            Priority::High => "high",
            Priority::Medium => "medium",
            Priority::Low => "low",
        }
    }

    fn string_to_priority(s: &str) -> Result<Priority, StorageError> {
        match s.to_lowercase().as_str() {
            "high" => Ok(Priority::High),
            "medium" => Ok(Priority::Medium),
            "low" => Ok(Priority::Low),
            _ => Err(StorageError::Storage(format!("Invalid priority: {}", s))),
        }
    }
}

impl Storage for SqliteStorage {
    fn save(&self, data: &StorageData) -> Result<(), StorageError> {
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| StorageError::Storage(format!("Failed to lock connection: {}", e)))?;

        let tx = conn
            .transaction()
            .map_err(|e| StorageError::Storage(format!("Failed to start transaction: {}", e)))?;

        // Clear existing data
        tx.execute("DELETE FROM tasks", [])?;
        tx.execute("DELETE FROM categories", [])?;

        // Insert categories
        for category in &data.categories {
            tx.execute(
                "INSERT INTO categories (id, name, created_at) VALUES (?1, ?2, ?3)",
                params![category.id, category.name, category.created_at.to_rfc3339(),],
            )
            .map_err(|e| StorageError::Storage(format!("Failed to insert category: {}", e)))?;
        }

        // Insert tasks
        for task in &data.tasks {
            tx.execute(
                "INSERT INTO tasks (id, title, category_id, completed, priority, created_at, updated_at) 
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    task.id,
                    task.title,
                    task.category_id,
                    task.completed,
                    Self::priority_to_string(&task.priority),
                    task.created_at.to_rfc3339(),
                    task.updated_at.to_rfc3339(),
                ],
            ).map_err(|e| StorageError::Storage(format!("Failed to insert task: {}", e)))?;
        }

        tx.commit()
            .map_err(|e| StorageError::Storage(format!("Failed to commit transaction: {}", e)))?;

        Ok(())
    }

    fn load(&self) -> Result<StorageData, StorageError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| StorageError::Storage(format!("Failed to lock connection: {}", e)))?;

        let mut categories = Vec::new();
        let mut tasks = Vec::new();

        // Load categories
        let mut stmt = conn
            .prepare("SELECT id, name, created_at FROM categories")
            .map_err(|e| {
                StorageError::Storage(format!("Failed to prepare categories query: {}", e))
            })?;

        let category_iter = stmt
            .query_map([], |row| {
                Ok(Category {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(2)?)
                        .map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                2,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?
                        .into(),
                })
            })
            .map_err(|e| StorageError::Storage(format!("Failed to query categories: {}", e)))?;

        for category in category_iter {
            categories.push(
                category.map_err(|e| {
                    StorageError::Storage(format!("Failed to read category: {}", e))
                })?,
            );
        }

        // Load tasks
        let mut stmt = conn
            .prepare("SELECT id, title, category_id, completed, priority, created_at, updated_at FROM tasks")
            .map_err(|e| StorageError::Storage(format!("Failed to prepare tasks query: {}", e)))?;

        let task_iter = stmt
            .query_map([], |row| {
                Ok(Task {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    category_id: row.get(2)?,
                    completed: row.get(3)?,
                    priority: Self::string_to_priority(&row.get::<_, String>(4)?).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            4,
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                        )
                    })?,
                    created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                        .map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                5,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?
                        .into(),
                    updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(6)?)
                        .map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                6,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?
                        .into(),
                })
            })
            .map_err(|e| StorageError::Storage(format!("Failed to query tasks: {}", e)))?;

        for task in task_iter {
            tasks.push(
                task.map_err(|e| StorageError::Storage(format!("Failed to read task: {}", e)))?,
            );
        }

        Ok(StorageData { tasks, categories })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_sqlite_storage() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(temp_file.path()).unwrap();

        // Test data
        let test_data = StorageData {
            tasks: vec![Task {
                id: 1,
                title: "Test Task".to_string(),
                category_id: 1,
                completed: false,
                priority: Priority::Medium,
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            }],
            categories: vec![Category {
                id: 1,
                name: "Test Category".to_string(),
                created_at: chrono::Utc::now(),
            }],
        };

        // Test save
        storage.save(&test_data).unwrap();

        // Test load
        let loaded_data = storage.load().unwrap();
        assert_eq!(loaded_data.tasks.len(), 1);
        assert_eq!(loaded_data.categories.len(), 1);
        assert_eq!(loaded_data.tasks[0].title, "Test Task");
        assert_eq!(loaded_data.categories[0].name, "Test Category");
        assert_eq!(loaded_data.tasks[0].priority, Priority::Medium);
    }

    #[test]
    fn test_empty_sqlite_storage() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(temp_file.path()).unwrap();

        let loaded_data = storage.load().unwrap();
        assert_eq!(loaded_data.tasks.len(), 0);
        assert_eq!(loaded_data.categories.len(), 0);
    }
}
