use super::{Storage, StorageError};
use crate::config::Config;
use std::path::Path;

#[derive(Debug)]
pub struct ConfigStorage {
    path: std::path::PathBuf,
}

impl ConfigStorage {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        Ok(Self {
            path: path.as_ref().to_path_buf(),
        })
    }
}

impl Storage for ConfigStorage {
    fn save(&self, data: &crate::models::StorageData) -> Result<(), StorageError> {
        // The config file holds only the config half of `StorageData` (tasks
        // and categories live in the separate data file). Cloning the whole
        // `Config` rather than rebuilding it field by field means a newly
        // added field - like the `default_categories_offered` first-run
        // marker - can't be silently dropped on save.
        let config = data.config.clone();

        // Create parent directories if they don't exist
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(&config)?;
        std::fs::write(&self.path, json)?;

        // Verify the write was successful by reading back
        let contents = std::fs::read_to_string(&self.path)?;
        let _: Config = serde_json::from_str(&contents)?;

        Ok(())
    }

    fn load(&self) -> Result<crate::models::StorageData, StorageError> {
        // No config file (fresh install) means nothing has been stored yet -
        // that's `Config::unset()`, not `Config::default()`. Returning the
        // resolved documented defaults here would make every field look
        // "explicitly set", and `config list` would lose the ability to
        // print the `*` markers that distinguish a default from a real
        // value.
        if !self.path.exists() {
            return Ok(crate::models::StorageData {
                version: 1,
                tasks: Vec::new(),
                categories: Vec::new(),
                config: Config::unset(),
                current_category: None,
                last_sync: chrono::Utc::now(),
            });
        }

        let contents = std::fs::read_to_string(&self.path)?;

        // An empty file is the same "nothing stored yet" case as a missing
        // one.
        if contents.trim().is_empty() {
            return Ok(crate::models::StorageData {
                version: 1,
                tasks: Vec::new(),
                categories: Vec::new(),
                config: Config::unset(),
                current_category: None,
                last_sync: chrono::Utc::now(),
            });
        }

        let config: Config = serde_json::from_str(&contents)?;
        Ok(crate::models::StorageData {
            version: 1,
            tasks: Vec::new(),
            categories: Vec::new(),
            config,
            current_category: None,
            last_sync: chrono::Utc::now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_config_storage() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");
        let storage = ConfigStorage::new(config_path).unwrap();

        let data = crate::models::StorageData {
            version: 1,
            tasks: vec![],
            categories: vec![],
            config: Config {
                deleted_task_lifespan: Some(7),
                storage_type: Some("json".to_string()),
                storage_path: Some("~/.config/trtodo".to_string()),
                default_category: Some("work".to_string()),
                default_priority: Some("medium".to_string()),
                default_categories_offered: Some(true),
            },
            current_category: None,
            last_sync: chrono::Utc::now(),
        };

        // Test save
        storage.save(&data).unwrap();

        // Test load
        let loaded_data = storage.load().unwrap();
        assert_eq!(loaded_data.config.deleted_task_lifespan, Some(7));
        assert_eq!(loaded_data.config.storage_type, Some("json".to_string()));
        assert_eq!(
            loaded_data.config.storage_path,
            Some("~/.config/trtodo".to_string())
        );
        assert_eq!(
            loaded_data.config.default_category,
            Some("work".to_string())
        );
        assert_eq!(
            loaded_data.config.default_priority,
            Some("medium".to_string())
        );
        // The first-run marker round-trips like any other field.
        assert_eq!(loaded_data.config.default_categories_offered, Some(true));
    }

    // Values unchanged from before this fix (a missing file already produced
    // `None` fields, since the old `#[derive(Default)]` also gave `None` for
    // every `Option` field) - but the *reason* changed: this now explicitly
    // exercises `Config::unset()`, not a `Config::default()` that happened
    // to coincide with it. Extended to also cover the empty-file branch,
    // which previously had no dedicated coverage of its own.
    #[test]
    fn test_empty_config_storage() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");
        let storage = ConfigStorage::new(&config_path).unwrap();

        // Case 1: no file at all (fresh install).
        let loaded_data = storage.load().unwrap();
        assert_eq!(loaded_data.config.deleted_task_lifespan, None);
        assert_eq!(loaded_data.config.storage_type, None);
        assert_eq!(loaded_data.config.storage_path, None);
        assert_eq!(loaded_data.config.default_category, None);
        assert_eq!(loaded_data.config.default_priority, None);

        // Case 2: an empty file - same "nothing stored yet" outcome.
        std::fs::write(&config_path, "").unwrap();
        let loaded_data = storage.load().unwrap();
        assert_eq!(loaded_data.config.deleted_task_lifespan, None);
        assert_eq!(loaded_data.config.storage_type, None);
        assert_eq!(loaded_data.config.storage_path, None);
        assert_eq!(loaded_data.config.default_category, None);
        assert_eq!(loaded_data.config.default_priority, None);
        // No first-run marker either way, which is what makes both of these
        // count as a first run.
        assert_eq!(loaded_data.config.default_categories_offered, None);
    }

    /// Regression test: writing a config with
    /// only one field set must not materialize explicit JSON `null`s for the
    /// rest, or the next `load()` would see those keys as *present* and
    /// never fall back to a default for them.
    #[test]
    fn test_save_omits_unset_fields_instead_of_writing_null() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");
        let storage = ConfigStorage::new(&config_path).unwrap();

        let mut data = storage.load().unwrap();
        assert_eq!(data.config.default_priority, None);
        data.config.default_priority = Some("high".to_string());
        storage.save(&data).unwrap();

        let contents = std::fs::read_to_string(&config_path).unwrap();
        assert!(
            !contents.contains("null"),
            "unset fields must be omitted, not written as null: {contents}"
        );

        let reloaded = storage.load().unwrap();
        assert_eq!(reloaded.config.default_priority, Some("high".to_string()));
        assert_eq!(reloaded.config.storage_type, None);
        assert_eq!(reloaded.config.storage_path, None);
        assert_eq!(reloaded.config.default_category, None);
        assert_eq!(reloaded.config.deleted_task_lifespan, None);
    }
}
