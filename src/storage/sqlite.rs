use super::Storage;
use super::StorageError;
use crate::category_manager::UNCATEGORIZED_ID;
use crate::models::{Category, Priority, StorageData, Task};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// The schema this build knows how to read and write.
///
/// Version history, so the number means something concrete:
///   - `1`: the original tables, with no `deleted_at` column on `tasks`.
///   - `2`: `tasks.deleted_at` added for soft deletion.
///
/// Bump this *and* teach `initialize_schema` how to get a database from the
/// previous version to the new one whenever the on-disk shape changes.
const SCHEMA_VERSION: i32 = 2;

/// Created and read before anything else, so `initialize_schema` can find out
/// what it is dealing with *before* it starts mutating tables. Deliberately
/// separate from `INIT_SCHEMA` below for that ordering alone.
const INIT_VERSION_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
);
"#;

const INIT_SCHEMA: &str = r#"
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

    /// Brings the database at hand up to `SCHEMA_VERSION`, or refuses to touch
    /// it if it cannot.
    ///
    /// What versioning here *is*: a guard rail plus one hand-written upgrade
    /// step. `schema_version` is read back before any table is touched, an
    /// unknown (newer) version is rejected outright with a message the user
    /// can act on, and a version-1 database is upgraded in place by adding
    /// the `deleted_at` column before the stored version is advanced.
    ///
    /// What it is *not*: a general migration system. There is no ordered
    /// `MIGRATIONS` list, no down-migrations, and no way to go from version N
    /// to N+2 other than by writing that step here by hand. `src/storage/
    /// migrations.rs` used to gesture at one but never held a single
    /// migration, and was deleted. Rather than resurrect it - or drop
    /// `schema_version` and lose the record of the v1 -> v2 change - the
    /// table was simply given the reader it never had.
    /// Designing the real thing is deferred until the schema settles - do not
    /// mistake the code below for it.
    fn initialize_schema(&self) -> Result<(), StorageError> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| StorageError::Storage(format!("Failed to lock connection: {}", e)))?;

        // Enable foreign key constraints
        conn.execute("PRAGMA foreign_keys = ON", [])
            .map_err(|e| StorageError::Storage(format!("Failed to enable foreign keys: {}", e)))?;

        // Read the stored version *first*, before any table is created or
        // altered: if this database came from a build we don't understand, we
        // want to have changed nothing by the time we bail out.
        conn.execute_batch(INIT_VERSION_TABLE).map_err(|e| {
            StorageError::Storage(format!("Failed to initialize schema version table: {}", e))
        })?;

        // `MAX(version)` rather than a bare `SELECT version`: it collapses an
        // empty table (a brand-new database, or one seeded by some earlier
        // build that never inserted a row) into a clean `None` instead of a
        // `QueryReturnedNoRows` error, and it is robust to a stray duplicate
        // row.
        let stored_version: Option<i32> = conn
            .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                row.get(0)
            })
            .map_err(|e| StorageError::Storage(format!("Failed to read schema version: {}", e)))?;

        // A version from the future is the one case we cannot handle: the
        // tables may hold columns, constraints, or encodings this build has
        // never heard of, and blundering on would either error somewhere far
        // less comprehensible or quietly write data back in the older shape.
        // Fail here, while the database is still untouched, and say exactly
        // what happened.
        if let Some(version) = stored_version {
            if version > SCHEMA_VERSION {
                return Err(StorageError::Storage(format!(
                    "this database uses schema version {}, but this build of trt only \
                     understands up to version {}; it was most likely written by a newer \
                     version of trt. Upgrade trt, or point storage.path at a different \
                     directory. The database has not been modified.",
                    version, SCHEMA_VERSION
                )));
            }
        }

        // Now create the tables. `CREATE TABLE IF NOT EXISTS` is a no-op
        // against a database that already has a `tasks` table from before
        // `deleted_at` existed, so it will NOT retroactively add the column -
        // that is handled explicitly below.
        conn.execute_batch(INIT_SCHEMA)
            .map_err(|e| StorageError::Storage(format!("Failed to initialize schema: {}", e)))?;

        // Migrate pre-existing `tasks` tables (schema version 1)
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

        // Record where we ended up. Written last, after the upgrade above has
        // actually succeeded, so a database that failed part way through is
        // never left claiming a version it doesn't have.
        match stored_version {
            None => {
                conn.execute(
                    "INSERT INTO schema_version (version) VALUES (?1)",
                    params![SCHEMA_VERSION],
                )
                .map_err(|e| {
                    StorageError::Storage(format!("Failed to set schema version: {}", e))
                })?;
            }
            Some(version) if version < SCHEMA_VERSION => {
                conn.execute(
                    "UPDATE schema_version SET version = ?1 WHERE version < ?1",
                    params![SCHEMA_VERSION],
                )
                .map_err(|e| {
                    StorageError::Storage(format!("Failed to update schema version: {}", e))
                })?;
            }
            // Already current. The `>` case was rejected above, so this arm
            // is exactly `version == SCHEMA_VERSION`: leave the row alone.
            Some(_) => {}
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

        // Seed the Uncategorized sentinel row before any task is inserted.
        //
        // `Uncategorized` (ID 0) is synthesized by `CategoryManager` rather
        // than stored - it cannot be created, renamed, or deleted, so it has
        // no business being in `data.categories`, and it never is. But the
        // `tasks.category_id` foreign key does not know that: a task filed
        // under Uncategorized references a row that, without this, does not
        // exist, and SQLite rejects the insert with "FOREIGN KEY constraint
        // failed". JSON has no such constraint, so the same data saved fine
        // there and only SQLite users hit it.
        //
        // That made every uncategorized task unstorable under SQLite - which
        // is now the *default* landing place for `add` with no `--category`
        // and a routine thing to carry across a backend switch. Rather than
        // drop the foreign key, which is genuinely
        // worth having for real categories, we give it the one row it is
        // missing. `load` filters this row back out (see there), so the
        // sentinel is an implementation detail of this backend and is never
        // visible to callers.
        tx.execute(
            "INSERT INTO categories (id, name, description, \"order\", created_at) \
             VALUES (?1, ?2, NULL, ?3, ?4)",
            // The name is immaterial - `load` filters this row out and
            // callers synthesize their own - but it is spelled the same way
            // so anyone opening the database with `sqlite3` sees something
            // recognizable rather than a mystery row.
            params![
                UNCATEGORIZED_ID,
                "Uncategorized",
                0i64,
                Utc::now().to_rfc3339()
            ],
        )
        .map_err(|e| {
            StorageError::Storage(format!("Failed to seed the Uncategorized category: {}", e))
        })?;

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
            let category = category
                .map_err(|e| StorageError::Storage(format!("Failed to read category: {}", e)))?;

            // Hide the Uncategorized sentinel `save` seeds to satisfy the
            // `tasks.category_id` foreign key. Callers synthesize their own
            // Uncategorized and treat a *stored* one as data corruption, so
            // letting it escape here would be visible in two ways that both
            // matter: `first_run` decides a store is untouched by asking
            // whether `get_all_categories` is empty, and it would
            // otherwise be copied into the other backend by a `storage.type`
            // switch. Filtering here keeps the sentinel local to
            // this backend, so both stay correct and JSON and SQLite continue
            // to load as exactly the same data.
            if category.id != UNCATEGORIZED_ID {
                categories.push(category);
            }
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
    /// same as every other task field.
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

    /// A task filed under the synthesized "Uncategorized" category (ID 0)
    /// must save and load like any other, and must not drag the sentinel row
    /// `save` seeds to satisfy the `tasks.category_id` foreign key back out
    /// with it.
    ///
    /// This is a regression test for a real defect: because "Uncategorized"
    /// is synthesized rather than stored, nothing satisfied that foreign key
    /// and SQLite rejected every such insert with "FOREIGN KEY constraint
    /// failed", while JSON - which has no constraint - accepted the identical
    /// data. It stayed latent while `add` required `--category`; it became
    /// the default path once `--category` was made optional with
    /// Uncategorized as the fallback, and a routine one for
    /// anyone carrying an uncategorized task across a `storage.type` switch.
    ///
    /// The second half is what keeps the sentinel from leaking: first-run
    /// detection asks whether `get_all_categories` is empty, so a
    /// visible sentinel would make a fresh SQLite store look established and
    /// silently suppress the setup offer.
    #[test]
    fn test_uncategorized_tasks_round_trip_without_leaking_the_sentinel() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("uncategorized.db");
        let storage = SqliteStorage::new(&db_path).unwrap();

        let mut data = create_test_data();
        data.tasks[0].category_id = UNCATEGORIZED_ID;
        storage.save(&data).unwrap();

        let loaded = storage.load().unwrap();
        let task = loaded.tasks.iter().find(|t| t.id == 1).unwrap();
        assert_eq!(task.category_id, UNCATEGORIZED_ID);

        // The sentinel is an implementation detail and must stay invisible.
        assert!(
            !loaded.categories.iter().any(|c| c.id == UNCATEGORIZED_ID),
            "the Uncategorized sentinel leaked out of the SQLite backend"
        );
        assert_eq!(loaded.categories.len(), data.categories.len());

        // A store holding only uncategorized tasks still reports no
        // categories, so first-run detection stays correct.
        let mut empty = create_test_data();
        empty.categories.clear();
        empty.tasks.iter_mut().for_each(|t| {
            t.category_id = UNCATEGORIZED_ID;
        });
        empty.current_category = None;
        storage.save(&empty).unwrap();
        assert!(storage.load().unwrap().categories.is_empty());
    }

    /// A database created before `deleted_at` existed (schema version 1) has
    /// a `tasks` table with no such column. Opening it must migrate the
    /// column in place rather than erroring - this is what
    /// `initialize_schema`'s `PRAGMA table_info` check guards.
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

    /// `schema_version` used to be written once at database creation and
    /// never read, so the number it held was decorative. It now has a
    /// reader. On a brand-new database the seeded value must be the
    /// version this build actually writes, and re-opening must leave exactly
    /// one row saying so - a second row (or a bumped value) would mean the
    /// bookkeeping drifts every time the app starts.
    #[test]
    fn test_schema_version_is_seeded_once_and_stays_put() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("versioned.db");

        let read_versions = |storage: &SqliteStorage| -> Vec<i32> {
            let conn = storage.conn.lock().unwrap();
            let mut stmt = conn.prepare("SELECT version FROM schema_version").unwrap();
            let rows: Vec<i32> = stmt
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(|v| v.unwrap())
                .collect();
            rows
        };

        let storage = SqliteStorage::new(&db_path).unwrap();
        assert_eq!(read_versions(&storage), vec![SCHEMA_VERSION]);
        drop(storage);

        let storage = SqliteStorage::new(&db_path).unwrap();
        assert_eq!(read_versions(&storage), vec![SCHEMA_VERSION]);
    }

    /// The reader's whole reason for existing: a database written
    /// by a *newer* build carries a version this code has never heard of. Its
    /// tables may hold columns or constraints we don't know about, so opening
    /// it must fail with a message the user can act on rather than half-work
    /// and write data back in an older shape.
    #[test]
    fn test_rejects_a_database_from_a_newer_build() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("from_the_future.db");

        {
            let conn = Connection::open(&db_path).unwrap();
            conn.execute_batch("CREATE TABLE schema_version (version INTEGER NOT NULL);")
                .unwrap();
            conn.execute(
                "INSERT INTO schema_version (version) VALUES (?1)",
                params![SCHEMA_VERSION + 1],
            )
            .unwrap();
        }

        let message = match SqliteStorage::new(&db_path) {
            Err(e) => e.to_string(),
            Ok(_) => {
                panic!("a database from a newer build must be rejected, not opened and written to")
            }
        };
        assert!(
            message.contains(&(SCHEMA_VERSION + 1).to_string())
                && message.contains(&SCHEMA_VERSION.to_string()),
            "the error should name both the version found and the version supported, got: {message}"
        );
        assert!(
            message.contains("newer version of trt"),
            "the error should explain *why* this happened, got: {message}"
        );

        // And it must have bailed out before touching anything: no tables were
        // created behind the user's back.
        let conn = Connection::open(&db_path).unwrap();
        let table_count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name IN ('tasks', 'categories', 'current_category')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            table_count, 0,
            "a rejected database must be left exactly as it was found"
        );
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
