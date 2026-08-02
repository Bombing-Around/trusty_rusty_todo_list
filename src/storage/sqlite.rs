use super::Storage;
use super::StorageError;
use crate::models::{Category, Priority, StorageData, Task};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[allow(dead_code)]
const SCHEMA_VERSION: i32 = 2;

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
    description TEXT,
    "order" INTEGER NOT NULL,
    created_at TEXT NOT NULL
);

-- Create tasks table
CREATE TABLE IF NOT EXISTS tasks (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT,
    category_id INTEGER NOT NULL,
    completed BOOLEAN NOT NULL DEFAULT 0,
    priority TEXT NOT NULL,
    due_date TEXT,
    "order" INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    FOREIGN KEY (category_id) REFERENCES categories(id)
);

-- Holds the persisted `category use` context. At most one row; absence of a
-- row means "no category selected".
CREATE TABLE IF NOT EXISTS current_category (
    id INTEGER PRIMARY KEY,
    category_id INTEGER NOT NULL
);
"#;

pub struct SqliteStorage {
    pub conn: Arc<Mutex<Connection>>,
}

#[allow(dead_code)]
impl SqliteStorage {
    pub fn new(path: &Path) -> Result<Self, StorageError> {
        let conn = Connection::open(path)
            .map_err(|e| StorageError::Storage(format!("Failed to open SQLite database: {}", e)))?;

        let storage = SqliteStorage {
            conn: Arc::new(Mutex::new(conn)),
        };

        // Initialize the schema
        storage.initialize_schema()?;

        Ok(storage)
    }

    fn initialize_schema(&self) -> Result<(), StorageError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| StorageError::Storage(format!("Failed to lock connection: {}", e)))?;

        // Enable foreign key constraints
        conn.execute("PRAGMA foreign_keys = ON", [])
            .map_err(|e| StorageError::Storage(format!("Failed to enable foreign keys: {}", e)))?;

        // First create the tables. `CREATE TABLE IF NOT EXISTS` is a no-op
        // against a database that already has a `tasks` table from before
        // `deleted_at` existed, so it will NOT retroactively add the column -
        // that is handled explicitly below.
        conn.execute_batch(INIT_SCHEMA)
            .map_err(|e| StorageError::Storage(format!("Failed to initialize schema: {}", e)))?;

        // Migrate pre-existing `tasks` tables (schema version 1, issue #29)
        // that predate the `deleted_at` column. `PRAGMA table_info` lets us
        // check for the column's presence directly instead of trying the
        // `ALTER TABLE` and swallowing a "duplicate column" error, so this
        // stays idempotent no matter how many times it runs.
        let has_deleted_at: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('tasks') WHERE name = 'deleted_at'")
            .and_then(|mut stmt| stmt.exists([]))
            .map_err(|e| {
                StorageError::Storage(format!("Failed to inspect tasks table schema: {}", e))
            })?;

        if !has_deleted_at {
            conn.execute("ALTER TABLE tasks ADD COLUMN deleted_at TEXT", [])
                .map_err(|e| {
                    StorageError::Storage(format!("Failed to add deleted_at column: {}", e))
                })?;
        }

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
        } else {
            // Bring an older database's stored version up to date now that
            // the migration above has run.
            conn.execute(
                "UPDATE schema_version SET version = ?1 WHERE version < ?1",
                params![SCHEMA_VERSION],
            )
            .map_err(|e| {
                StorageError::Storage(format!("Failed to update schema version: {}", e))
            })?;
        }

        Ok(())
    }

    pub fn priority_to_string(priority: Priority) -> String {
        match priority {
            Priority::High => "high".to_string(),
            Priority::Medium => "medium".to_string(),
            Priority::Low => "low".to_string(),
        }
    }

    pub fn string_to_priority(s: &str) -> Result<Priority, StorageError> {
        match s {
            "high" => Ok(Priority::High),
            "medium" => Ok(Priority::Medium),
            "low" => Ok(Priority::Low),
            _ => Err(StorageError::InvalidData(
                "Invalid priority value".to_string(),
            )),
        }
    }
}

impl Storage for SqliteStorage {
    fn save(&self, data: &StorageData) -> Result<(), StorageError> {
        data.validate()?;

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
        tx.execute("DELETE FROM current_category", [])?;

        // Persist the selected category context, if any
        if let Some(category_id) = data.current_category {
            tx.execute(
                "INSERT INTO current_category (id, category_id) VALUES (1, ?1)",
                params![category_id],
            )
            .map_err(|e| {
                StorageError::Storage(format!("Failed to save current category: {}", e))
            })?;
        }

        // Insert categories
        for category in &data.categories {
            tx.execute(
                "INSERT INTO categories (id, name, description, \"order\", created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    category.id,
                    category.name,
                    category.description,
                    category.order,
                    category.created_at.to_rfc3339(),
                ],
            )
            .map_err(|e| StorageError::Storage(format!("Failed to insert category: {}", e)))?;
        }

        // Insert tasks
        for task in &data.tasks {
            tx.execute(
                "INSERT INTO tasks (id, title, description, category_id, completed, priority, due_date, \"order\", created_at, updated_at, deleted_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    task.id,
                    task.title,
                    task.description,
                    task.category_id,
                    task.completed,
                    Self::priority_to_string(task.priority),
                    task.due_date.map(|d| d.to_rfc3339()),
                    task.order,
                    task.created_at.to_rfc3339(),
                    task.updated_at.to_rfc3339(),
                    task.deleted_at.map(|d| d.to_rfc3339()),
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
            .prepare("SELECT id, name, description, \"order\", created_at FROM categories")
            .map_err(|e| {
                StorageError::Storage(format!("Failed to prepare categories query: {}", e))
            })?;

        let category_iter = stmt
            .query_map([], |row| {
                Ok(Category {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    order: row.get(3)?,
                    created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                        .map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                4,
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
            .prepare("SELECT id, title, description, category_id, completed, priority, due_date, \"order\", created_at, updated_at, deleted_at FROM tasks")
            .map_err(|e| StorageError::Storage(format!("Failed to prepare tasks query: {}", e)))?;

        let task_iter = stmt
            .query_map([], |row| {
                Ok(Task {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    description: row.get(2)?,
                    category_id: row.get(3)?,
                    completed: row.get(4)?,
                    priority: Self::string_to_priority(&row.get::<_, String>(5)?).map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            5,
                            rusqlite::types::Type::Text,
                            Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
                        )
                    })?,
                    due_date: row
                        .get::<_, Option<String>>(6)?
                        .map(|s| {
                            let s = s.to_string();
                            chrono::DateTime::parse_from_rfc3339(&s)
                                .map(|dt| dt.with_timezone(&chrono::Utc))
                                .map_err(|e| {
                                    rusqlite::Error::FromSqlConversionFailure(
                                        6,
                                        rusqlite::types::Type::Text,
                                        Box::new(e),
                                    )
                                })
                        })
                        .transpose()?,
                    order: row.get(7)?,
                    created_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(8)?)
                        .map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                8,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?
                        .into(),
                    updated_at: chrono::DateTime::parse_from_rfc3339(&row.get::<_, String>(9)?)
                        .map_err(|e| {
                            rusqlite::Error::FromSqlConversionFailure(
                                9,
                                rusqlite::types::Type::Text,
                                Box::new(e),
                            )
                        })?
                        .into(),
                    deleted_at: row
                        .get::<_, Option<String>>(10)?
                        .map(|s| {
                            chrono::DateTime::parse_from_rfc3339(&s)
                                .map(|dt| dt.with_timezone(&chrono::Utc))
                                .map_err(|e| {
                                    rusqlite::Error::FromSqlConversionFailure(
                                        10,
                                        rusqlite::types::Type::Text,
                                        Box::new(e),
                                    )
                                })
                        })
                        .transpose()?,
                })
            })
            .map_err(|e| StorageError::Storage(format!("Failed to query tasks: {}", e)))?;

        for task in task_iter {
            tasks.push(
                task.map_err(|e| StorageError::Storage(format!("Failed to read task: {}", e)))?,
            );
        }

        // Load the persisted category context (at most one row)
        let current_category = conn
            .query_row(
                "SELECT category_id FROM current_category LIMIT 1",
                [],
                |row| row.get::<_, u64>(0),
            )
            .optional()
            .map_err(|e| {
                StorageError::Storage(format!("Failed to read current category: {}", e))
            })?;

        let data = StorageData {
            version: 1,
            tasks,
            categories,
            // Vestigial field - see the comment on `StorageData::new`. SQLite
            // doesn't store config data either, so "nothing stored" is the
            // honest value.
            config: crate::config::Config::unset(),
            current_category,
            last_sync: Utc::now(),
        };

        // Validate loaded data
        data.validate()?;

        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_data() -> StorageData {
        let mut data = StorageData::new();

        // Create test categories with unique IDs
        let mut work =
            Category::new("Work".to_string(), Some("Work related tasks".to_string())).unwrap();
        work.id = 1;
        let mut personal =
            Category::new("Personal".to_string(), Some("Personal tasks".to_string())).unwrap();
        personal.id = 2;

        data.categories.push(work.clone());
        data.categories.push(personal.clone());

        // Create test tasks with unique IDs
        let mut task1 = Task::new(
            "Complete project".to_string(),
            work.id,
            Some("Finish the todo list project".to_string()),
            Priority::High,
        )
        .unwrap();
        task1.id = 1;

        let mut task2 = Task::new(
            "Buy groceries".to_string(),
            personal.id,
            Some("Get milk and bread".to_string()),
            Priority::Medium,
        )
        .unwrap();
        task2.id = 2;

        data.tasks.push(task1);
        data.tasks.push(task2);

        data
    }

    #[test]
    fn test_sqlite_storage() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let storage = SqliteStorage::new(&db_path).unwrap();

        let test_data = create_test_data();
        storage.save(&test_data).unwrap();

        let loaded_data = storage.load().unwrap();
        assert_eq!(loaded_data.tasks.len(), 2);
        assert_eq!(loaded_data.categories.len(), 2);

        // Verify task data
        let task = loaded_data.tasks.first().unwrap();
        assert_eq!(task.title, "Complete project");
        assert_eq!(task.priority, Priority::High);
        assert!(task.description.is_some());
        assert_eq!(
            task.description.as_ref().unwrap(),
            "Finish the todo list project"
        );

        // Verify category data
        let category = loaded_data.categories.first().unwrap();
        assert_eq!(category.name, "Work");
        assert!(category.description.is_some());
        assert_eq!(category.description.as_ref().unwrap(), "Work related tasks");
    }

    #[test]
    fn test_empty_sqlite_storage() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("empty.db");
        let storage = SqliteStorage::new(&db_path).unwrap();

        let loaded_data = storage.load().unwrap();
        assert_eq!(loaded_data.tasks.len(), 0);
        assert_eq!(loaded_data.categories.len(), 0);
    }

    #[test]
    fn test_invalid_priority() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("invalid.db");
        let storage = SqliteStorage::new(&db_path).unwrap();

        {
            let conn = storage.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO categories (id, name, description, \"order\", created_at) VALUES (1, 'Test', NULL, 0, ?)",
                params![Utc::now().to_rfc3339()],
            )
            .unwrap();

            conn.execute(
                "INSERT INTO tasks (id, title, description, category_id, completed, priority, due_date, \"order\", created_at, updated_at) 
                 VALUES (1, 'Test', NULL, 1, 0, 'Invalid', NULL, 0, ?, ?)",
                params![Utc::now().to_rfc3339(), Utc::now().to_rfc3339()],
            )
            .unwrap();
        } // Lock is released here when conn goes out of scope

        let result = storage.load();
        assert!(result.is_err());
    }

    /// The `category use` context must survive a save/load cycle in the SQLite
    /// backend too, not just JSON.
    #[test]
    fn test_current_category_round_trip() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("context.db");
        let storage = SqliteStorage::new(&db_path).unwrap();

        let mut data = create_test_data();
        assert_eq!(data.current_category, None);
        data.current_category = Some(2);
        storage.save(&data).unwrap();

        assert_eq!(storage.load().unwrap().current_category, Some(2));

        // ... and clearing it round-trips too.
        let mut data = storage.load().unwrap();
        data.current_category = None;
        storage.save(&data).unwrap();
        assert_eq!(storage.load().unwrap().current_category, None);
    }

    /// `deleted_at` must survive a save/load cycle in the SQLite backend,
    /// same as every other task field (issue #29).
    #[test]
    fn test_deleted_at_round_trip() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("deleted_at.db");
        let storage = SqliteStorage::new(&db_path).unwrap();

        let mut data = create_test_data();
        data.tasks[0].soft_delete();
        storage.save(&data).unwrap();

        let loaded = storage.load().unwrap();
        let deleted_task = loaded.tasks.iter().find(|t| t.id == 1).unwrap();
        assert!(deleted_task.deleted_at.is_some());
        assert!(deleted_task.is_deleted());

        let live_task = loaded.tasks.iter().find(|t| t.id == 2).unwrap();
        assert!(live_task.deleted_at.is_none());

        // ... and restoring round-trips too.
        let mut data = loaded;
        data.tasks[0].restore();
        storage.save(&data).unwrap();
        assert!(storage.load().unwrap().tasks[0].deleted_at.is_none());
    }

    /// A database created before `deleted_at` existed (schema version 1) has
    /// a `tasks` table with no such column. Opening it must migrate the
    /// column in place rather than erroring - this is what
    /// `initialize_schema`'s `PRAGMA table_info` check guards (issue #29).
    #[test]
    fn test_migrates_pre_existing_database_without_deleted_at_column() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("legacy_schema.db");

        // Build a schema-version-1 database by hand, without deleted_at.
        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch(
                r#"
                CREATE TABLE schema_version (version INTEGER NOT NULL);
                INSERT INTO schema_version (version) VALUES (1);

                CREATE TABLE categories (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL,
                    description TEXT,
                    "order" INTEGER NOT NULL,
                    created_at TEXT NOT NULL
                );

                CREATE TABLE tasks (
                    id INTEGER PRIMARY KEY,
                    title TEXT NOT NULL,
                    description TEXT,
                    category_id INTEGER NOT NULL,
                    completed BOOLEAN NOT NULL DEFAULT 0,
                    priority TEXT NOT NULL,
                    due_date TEXT,
                    "order" INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    FOREIGN KEY (category_id) REFERENCES categories(id)
                );

                CREATE TABLE current_category (
                    id INTEGER PRIMARY KEY,
                    category_id INTEGER NOT NULL
                );
                "#,
            )
            .unwrap();

            conn.execute(
                "INSERT INTO categories (id, name, description, \"order\", created_at) VALUES (1, 'Work', NULL, 1, ?)",
                params![Utc::now().to_rfc3339()],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO tasks (id, title, description, category_id, completed, priority, due_date, \"order\", created_at, updated_at)
                 VALUES (1, 'Pre-existing task', NULL, 1, 0, 'high', NULL, 0, ?, ?)",
                params![Utc::now().to_rfc3339(), Utc::now().to_rfc3339()],
            )
            .unwrap();
        }

        // Opening through SqliteStorage must migrate the column in and still
        // load the pre-existing row (as "not deleted"), not error.
        let storage = SqliteStorage::new(&db_path).unwrap();
        let data = storage.load().unwrap();
        assert_eq!(data.tasks.len(), 1);
        assert_eq!(data.tasks[0].title, "Pre-existing task");
        assert_eq!(data.tasks[0].deleted_at, None);

        // Running the migration again (e.g. re-opening the same DB) must be
        // idempotent - it must not error trying to re-add the column.
        drop(storage);
        let storage = SqliteStorage::new(&db_path).unwrap();
        assert!(storage.load().is_ok());

        // The stored schema version was brought up to date.
        let version: i32 = storage
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn test_missing_foreign_key() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("foreign.db");
        let storage = SqliteStorage::new(&db_path).unwrap();

        let result = {
            let conn = storage.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO tasks (id, title, description, category_id, completed, priority, due_date, \"order\", created_at, updated_at) 
                 VALUES (1, 'Test', NULL, 999, 0, 'High', NULL, 0, ?, ?)",
                params![Utc::now().to_rfc3339(), Utc::now().to_rfc3339()],
            )
        }; // Lock is released here

        assert!(result.is_err());
    }
}
