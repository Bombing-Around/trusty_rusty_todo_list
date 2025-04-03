//! Database migration system for SQLite storage
//!
//! This module provides a migration system that allows for version-controlled schema changes
//! in the SQLite database. It supports both forward migrations (up) and backward migrations (down).
//!
//! # Usage
//!
//! To add a new migration:
//!
//! 1. Add a new migration to the `MIGRATIONS` array:
//! ```rust
//! pub const MIGRATIONS: &[Migration] = &[
//!     Migration {
//!         version: 2,
//!         up: "ALTER TABLE tasks ADD COLUMN due_date TEXT;",
//!         down: "ALTER TABLE tasks DROP COLUMN due_date;",
//!     },
//! ];
//! ```
//!
//! 2. The migration system will automatically:
//!    - Detect the current schema version
//!    - Apply any pending migrations in sequence
//!    - Handle rollbacks if needed
//!    - Maintain data integrity using transactions
//!
//! # Migration Guidelines
//!
//! 1. Each migration should have a unique version number
//! 2. Migrations should be idempotent (safe to run multiple times)
//! 3. Down migrations should exactly reverse the changes made in the up migration
//! 4. Use transactions to ensure data consistency
//! 5. Test both up and down migrations thoroughly
//!
//! # Example
//!
//! ```rust
//! Migration {
//!     version: 2,
//!     up: r#"
//!         -- Add new column with default value
//!         ALTER TABLE tasks ADD COLUMN due_date TEXT;
//!         -- Update existing rows with a default value
//!         UPDATE tasks SET due_date = datetime('now') WHERE due_date IS NULL;
//!     "#,
//!     down: "ALTER TABLE tasks DROP COLUMN due_date;",
//! }
//! ```

use super::StorageError;
use rusqlite::{Connection, Transaction};

/// Represents a database migration with up and down SQL statements
#[derive(Debug)]
#[allow(dead_code)]
pub struct Migration {
    /// The version number of this migration
    pub version: i32,
    /// SQL statements to apply this migration
    pub up: &'static str,
    /// SQL statements to reverse this migration
    pub down: &'static str,
}

/// List of all database migrations in order of application
pub const MIGRATIONS: &[Migration] = &[
    // Add future migrations here
    // Example:
    // Migration {
    //     version: 2,
    //     up: "ALTER TABLE tasks ADD COLUMN due_date TEXT;",
    //     down: "ALTER TABLE tasks DROP COLUMN due_date;",
    // },
];

/// Get the current schema version from the database
pub fn get_current_version(conn: &Connection) -> Result<i32, StorageError> {
    let version: i32 = conn
        .query_row("SELECT version FROM schema_version", [], |row| row.get(0))
        .map_err(|e| StorageError::Storage(format!("Failed to get schema version: {}", e)))?;
    Ok(version)
}

/// Apply any pending migrations to the database
///
/// This function will:
/// 1. Get the current schema version
/// 2. Find any migrations with a higher version number
/// 3. Apply those migrations in sequence
/// 4. Update the schema version after each successful migration
///
/// All migrations are applied within a transaction to ensure data consistency.
pub fn apply_migrations(conn: &mut Connection) -> Result<(), StorageError> {
    let current_version = get_current_version(conn)?;
    let latest_version = MIGRATIONS.last().map(|m| m.version).unwrap_or(0);

    if current_version < latest_version {
        let tx = conn
            .transaction()
            .map_err(|e| StorageError::Storage(format!("Failed to start transaction: {}", e)))?;

        for migration in MIGRATIONS.iter().filter(|m| m.version > current_version) {
            apply_migration(&tx, migration)?;
        }

        tx.commit()
            .map_err(|e| StorageError::Storage(format!("Failed to commit transaction: {}", e)))?;
    }

    Ok(())
}

/// Apply a single migration
fn apply_migration(tx: &Transaction, migration: &Migration) -> Result<(), StorageError> {
    // Execute the up migration
    tx.execute_batch(migration.up).map_err(|e| {
        StorageError::Storage(format!(
            "Failed to apply migration {}: {}",
            migration.version, e
        ))
    })?;

    // Update the schema version
    tx.execute(
        "UPDATE schema_version SET version = ?1",
        [migration.version],
    )
    .map_err(|e| {
        StorageError::Storage(format!(
            "Failed to update schema version to {}: {}",
            migration.version, e
        ))
    })?;

    Ok(())
}

/// Rollback migrations to a specific version
///
/// This function will:
/// 1. Get the current schema version
/// 2. Find any migrations with a version higher than the target
/// 3. Apply the down migrations in reverse order
/// 4. Update the schema version after each successful rollback
///
/// All rollbacks are applied within a transaction to ensure data consistency.
#[allow(dead_code)]
pub fn rollback_migrations(conn: &mut Connection, target_version: i32) -> Result<(), StorageError> {
    let current_version = get_current_version(conn)?;
    if current_version > target_version {
        let tx = conn
            .transaction()
            .map_err(|e| StorageError::Storage(format!("Failed to start transaction: {}", e)))?;

        // Special case: if rolling back to version 0 and current version is 1,
        // just update the version number since version 1 is the initial schema
        if target_version == 0 && current_version == 1 {
            tx.execute("UPDATE schema_version SET version = 0", [])
                .map_err(|e| {
                    StorageError::Storage(format!("Failed to update schema version to 0: {}", e))
                })?;
        } else {
            // Normal case: apply down migrations in reverse order
            for migration in MIGRATIONS
                .iter()
                .filter(|m| m.version > target_version && m.version <= current_version)
                .rev()
            {
                rollback_migration(&tx, migration)?;
            }
        }

        tx.commit()
            .map_err(|e| StorageError::Storage(format!("Failed to commit transaction: {}", e)))?;
    }

    Ok(())
}

/// Rollback a single migration
#[allow(dead_code)]
fn rollback_migration(tx: &Transaction, migration: &Migration) -> Result<(), StorageError> {
    // Execute the down migration
    tx.execute_batch(migration.down).map_err(|e| {
        StorageError::Storage(format!(
            "Failed to rollback migration {}: {}",
            migration.version, e
        ))
    })?;

    // Update the schema version
    tx.execute(
        "UPDATE schema_version SET version = ?1",
        [migration.version - 1],
    )
    .map_err(|e| {
        StorageError::Storage(format!(
            "Failed to update schema version to {}: {}",
            migration.version - 1,
            e
        ))
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_migration_system() {
        let temp_file = NamedTempFile::new().unwrap();
        let mut conn = Connection::open(temp_file.path()).unwrap();

        // Initialize schema version table
        conn.execute("CREATE TABLE schema_version (version INTEGER NOT NULL)", [])
            .unwrap();
        conn.execute("INSERT INTO schema_version (version) VALUES (1)", [])
            .unwrap();

        // Test applying migrations
        apply_migrations(&mut conn).unwrap();
        let version = get_current_version(&conn).unwrap();
        assert_eq!(
            version, 1,
            "Version should remain 1 as there are no migrations yet"
        );

        // Test rolling back migrations
        rollback_migrations(&mut conn, 0).unwrap();
        let version = get_current_version(&conn).unwrap();
        assert_eq!(version, 0, "Version should be 0 after rolling back");

        // Add a test migration
        const TEST_MIGRATION: Migration = Migration {
            version: 2,
            up: "CREATE TABLE test (id INTEGER PRIMARY KEY);",
            down: "DROP TABLE test;",
        };

        // Apply the test migration
        let tx = conn.transaction().unwrap();
        apply_migration(&tx, &TEST_MIGRATION).unwrap();
        tx.commit().unwrap();

        let version = get_current_version(&conn).unwrap();
        assert_eq!(version, 2, "Version should be 2 after applying migration");

        // Rollback the test migration
        let tx = conn.transaction().unwrap();
        rollback_migration(&tx, &TEST_MIGRATION).unwrap();
        tx.commit().unwrap();

        let version = get_current_version(&conn).unwrap();
        assert_eq!(
            version, 1,
            "Version should be 1 after rolling back migration"
        );
    }
}
