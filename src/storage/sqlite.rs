use super::migrations::apply_migrations;
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
        let mut conn = self
            .conn
            .lock()
            .map_err(|e| StorageError::Storage(format!("Failed to lock connection: {}", e)))?;

        // Enable foreign key constraints
        conn.execute("PRAGMA foreign_keys = ON", [])
            .map_err(|e| StorageError::Storage(format!("Failed to enable foreign keys: {}", e)))?;

        // First create the tables
        conn.execute_batch(INIT_SCHEMA)
            .map_err(|e| StorageError::Storage(format!("Failed to initialize schema: {}", e)))?;

        // Check if schema_version table exists and has a version
        let version_exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM schema_version WHERE version IS NOT NULL)",
                [],
                |row| row.get(0),
            )
            .map_err(|e| StorageError::Storage(format!("Failed to check schema version: {}", e)))?;

        if !version_exists {
            // Set initial schema version
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![SCHEMA_VERSION],
            )
            .map_err(|e| StorageError::Storage(format!("Failed to set schema version: {}", e)))?;
        }

        // Apply any pending migrations
        apply_migrations(&mut conn)?;

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
    use crate::storage::test_utils::*;
    use std::thread;
    use std::time::Duration;

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

    #[test]
    fn test_invalid_priority() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(temp_file.path()).unwrap();

        // Create a category first
        {
            let conn = storage.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO categories (id, name, created_at) VALUES (1, 'Test Category', '2024-01-01T00:00:00Z')",
                [],
            ).unwrap();

            // Try to insert a task with invalid priority directly into SQLite
            conn.execute(
                "INSERT INTO tasks (id, title, category_id, completed, priority, created_at, updated_at) 
                 VALUES (1, 'Test Task', 1, 0, 'INVALID_PRIORITY', '2024-01-01T00:00:00Z', '2024-01-01T00:00:00Z')",
                [],
            ).unwrap();
        }

        // Attempting to load should fail due to invalid priority
        let result = storage.load();
        assert!(result.is_err());
        if let Err(e) = result {
            println!("Invalid priority error: {}", e);
            assert!(format!("{}", e).contains("Invalid priority: INVALID_PRIORITY"));
        }
    }

    #[test]
    fn test_missing_foreign_key() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(temp_file.path()).unwrap();

        // Try to insert a task with non-existent category_id
        let conn = storage.conn.lock().unwrap();
        conn.execute("PRAGMA foreign_keys = ON", []).unwrap();
        let result = conn.execute(
            "INSERT INTO tasks (id, title, category_id, completed, priority, created_at, updated_at) 
             VALUES (1, 'Test Task', 999, 0, 'medium', datetime('now'), datetime('now'))",
            [],
        );

        // SQLite should enforce foreign key constraint
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(format!("{}", e).contains("FOREIGN KEY constraint failed"));
        }
    }

    #[test]
    fn test_data_corruption() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(temp_file.path()).unwrap();

        // Create a category first
        {
            let conn = storage.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO categories (id, name, created_at) VALUES (1, 'Test Category', '2024-01-01T00:00:00Z')",
                [],
            ).unwrap();

            // Insert task with corrupted datetime format
            conn.execute(
                "INSERT INTO tasks (id, title, category_id, completed, priority, created_at, updated_at) 
                 VALUES (1, 'Test Task', 1, 0, 'medium', 'not-a-date', 'not-a-date')",
                [],
            ).unwrap();
        }

        // Attempting to load should fail due to corrupted datetime
        let result = storage.load();
        assert!(result.is_err());
        if let Err(e) = result {
            println!("Data corruption error: {}", e);
            let err_msg = format!("{}", e);
            assert!(err_msg.contains("input contains invalid characters"));
        }
    }

    #[test]
    fn test_concurrent_access() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(temp_file.path()).unwrap();
        let storage_clone = SqliteStorage::new(temp_file.path()).unwrap();

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

        // Spawn a thread that saves data
        let handle = thread::spawn(move || {
            storage.save(&test_data).unwrap();
        });

        // Wait a bit to ensure the other thread has started
        thread::sleep(Duration::from_millis(100));

        // Try to load data from another connection while saving
        let result = storage_clone.load();
        assert!(result.is_ok());

        // Wait for the save thread to complete
        handle.join().unwrap();

        // Verify the data was saved correctly
        let loaded_data = storage_clone.load().unwrap();
        assert_eq!(loaded_data.tasks.len(), 1);
        assert_eq!(loaded_data.categories.len(), 1);
    }

    #[test]
    fn test_schema_version() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(temp_file.path()).unwrap();

        // Verify schema version was set correctly
        let conn = storage.conn.lock().unwrap();
        let version: i32 = conn
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_large_data_set() {
        let temp_file = NamedTempFile::new().unwrap();
        let storage = SqliteStorage::new(temp_file.path()).unwrap();

        // Create a large dataset
        let mut categories = Vec::new();
        let mut tasks = Vec::new();
        let now = chrono::Utc::now();

        // Create 1000 categories
        for i in 0..1000 {
            categories.push(Category {
                id: i,
                name: format!("Category {}", i),
                created_at: now,
            });
        }

        // Create 10000 tasks
        for i in 0..10000 {
            tasks.push(Task {
                id: i,
                title: format!("Task {}", i),
                category_id: i % 1000, // Distribute tasks across categories
                completed: i % 2 == 0,
                priority: match i % 3 {
                    0 => Priority::High,
                    1 => Priority::Medium,
                    _ => Priority::Low,
                },
                created_at: now,
                updated_at: now,
            });
        }

        let test_data = StorageData { tasks, categories };

        // Test saving large dataset
        storage.save(&test_data).unwrap();

        // Test loading large dataset
        let loaded_data = storage.load().unwrap();
        assert_eq!(loaded_data.categories.len(), 1000);
        assert_eq!(loaded_data.tasks.len(), 10000);
    }
}
