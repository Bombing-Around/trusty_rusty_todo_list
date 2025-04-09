use crate::models::{Category, Priority, StorageData, Task};
use crate::config::Config;
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::Mutex;
use chrono::Utc;
use super::{Storage, StorageError};
use shellexpand;

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
    FOREIGN KEY (category_id) REFERENCES categories(id)
);
"#;

pub struct SqliteStorage {
    conn: Mutex<Connection>,
}

impl SqliteStorage {
    pub fn new(config: Config) -> Result<Self, StorageError> {
        let path = config.storage_path
            .ok_or_else(|| StorageError::Storage("Storage path not configured".to_string()))?;
        let path = PathBuf::from(shellexpand::tilde(&path).to_string());
        let conn = Connection::open(&path)
            .map_err(|e| StorageError::Storage(format!("Failed to open database: {}", e)))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn get_connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>, StorageError> {
        self.conn
            .lock()
            .map_err(|e| StorageError::Storage(format!("Failed to lock connection: {}", e)))
    }

    fn init_tables(&self, conn: &Connection) -> Result<(), StorageError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS tasks (
                id INTEGER PRIMARY KEY,
                title TEXT NOT NULL,
                description TEXT,
                category_id INTEGER NOT NULL,
                completed BOOLEAN NOT NULL,
                priority TEXT NOT NULL,
                due_date TEXT,
                \"order\" INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| StorageError::Storage(format!("Failed to create tasks table: {}", e)))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS categories (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                description TEXT,
                \"order\" INTEGER NOT NULL,
                created_at TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| StorageError::Storage(format!("Failed to create categories table: {}", e)))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS config (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| StorageError::Storage(format!("Failed to create config table: {}", e)))?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS current_category (
                id INTEGER PRIMARY KEY,
                category_id INTEGER NOT NULL
            )",
            [],
        )
        .map_err(|e| StorageError::Storage(format!("Failed to create current_category table: {}", e)))?;

        Ok(())
    }

    fn load_tasks(&self, conn: &Connection) -> Result<Vec<Task>, StorageError> {
        let mut tasks = Vec::new();
        let mut stmt = conn
            .prepare("SELECT id, title, description, category_id, completed, priority, due_date, \"order\", created_at, updated_at FROM tasks")
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
                })
            })
            .map_err(|e| StorageError::Storage(format!("Failed to query tasks: {}", e)))?;

        for task in task_iter {
            tasks.push(
                task.map_err(|e| StorageError::Storage(format!("Failed to read task: {}", e)))?,
            );
        }

        Ok(tasks)
    }

    fn load_categories(&self, conn: &Connection) -> Result<Vec<Category>, StorageError> {
        let mut categories = Vec::new();
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

        Ok(categories)
    }

    fn load_config(&self, conn: &Connection) -> Result<Option<Config>, StorageError> {
        let mut config = Config::with_defaults();
        let mut stmt = conn
            .prepare("SELECT key, value FROM config")
            .map_err(|e| StorageError::Storage(format!("Failed to prepare config query: {}", e)))?;

        let rows = stmt
            .query_map([], |row| {
                let key: String = row.get(0)?;
                let value: String = row.get(1)?;
                Ok((key, value))
            })
            .map_err(|e| StorageError::Storage(format!("Failed to query config: {}", e)))?;

        for row in rows {
            let (key, value) = row
                .map_err(|e| StorageError::Storage(format!("Failed to read config row: {}", e)))?;
            match key.as_str() {
                "deleted_task_lifespan" => {
                    config.deleted_task_lifespan = Some(value.parse().map_err(|e| {
                        StorageError::Storage(format!("Invalid deleted_task_lifespan value: {}", e))
                    })?);
                }
                "storage_type" => config.storage_type = Some(value),
                "storage_path" => config.storage_path = Some(value),
                "default_category" => config.default_category = Some(value),
                "default_priority" => config.default_priority = Some(value),
                _ => {}
            }
        }

        Ok(Some(config))
    }

    fn load_current_category(&self, conn: &Connection) -> Result<Option<u64>, StorageError> {
        let mut stmt = conn
            .prepare("SELECT category_id FROM current_category LIMIT 1")
            .map_err(|e| {
                StorageError::Storage(format!("Failed to prepare current_category query: {}", e))
            })?;

        let mut rows = stmt
            .query_map([], |row| row.get::<_, u64>(0))
            .map_err(|e| {
                StorageError::Storage(format!("Failed to query current_category: {}", e))
            })?;

        if let Some(row) = rows.next() {
            Ok(Some(
                row.map_err(|e| {
                    StorageError::Storage(format!("Failed to read current_category: {}", e))
                })?,
            ))
        } else {
            Ok(None)
        }
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

        let mut conn = self.get_connection()?;

        let tx = conn.transaction()?;

        // Clear existing data
        tx.execute("DELETE FROM tasks", [])?;
        tx.execute("DELETE FROM categories", [])?;
        tx.execute("DELETE FROM config", [])?;
        tx.execute("DELETE FROM current_category", [])?;

        // Save categories
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
            )?;
        }

        // Save tasks
        for task in &data.tasks {
            tx.execute(
                "INSERT INTO tasks (id, title, description, category_id, completed, priority, due_date, \"order\", created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    task.id,
                    task.title,
                    task.description,
                    task.category_id,
                    task.completed,
                    Self::priority_to_string(task.priority.clone()),
                    task.due_date.map(|dt| dt.to_rfc3339()),
                    task.order,
                    task.created_at.to_rfc3339(),
                    task.updated_at.to_rfc3339(),
                ],
            )?;
        }

        tx.commit()?;

        Ok(())
    }

    fn load(&self) -> Result<StorageData, StorageError> {
        let conn = self.get_connection()?;

        // Initialize tables if they don't exist
        self.init_tables(&conn)?;

        // Load tasks
        let tasks = self.load_tasks(&conn)?;
        let categories = self.load_categories(&conn)?;
        let config = self.load_config(&conn)?;
        let current_category = self.load_current_category(&conn)?;

        Ok(StorageData {
            version: 1,
            tasks,
            categories,
            config: config.unwrap_or_else(Config::default),
            current_category,
            last_sync: Utc::now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_sqlite_storage() {
        let temp_dir = tempfile::tempdir().unwrap();
        let storage_path = temp_dir.path().join("tasks.db");
        let mut config = Config::default();
        config.storage_path = Some(storage_path.to_str().unwrap().to_string());
        let storage = SqliteStorage::new(config);
        assert!(storage.is_ok());
    }
}
