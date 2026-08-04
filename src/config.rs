use crate::models::StorageError;
use crate::storage::{config::ConfigStorage, Storage};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("Invalid key: {0}")]
    InvalidKey(String),
}

impl From<StorageError> for ConfigError {
    fn from(error: StorageError) -> Self {
        ConfigError::Storage(error.to_string())
    }
}

const VALID_STORAGE_TYPES: &[&str] = &["json", "sqlite"];
const VALID_PRIORITIES: &[&str] = &["high", "medium", "low"];

fn validate_storage_path(path: &str) -> Result<PathBuf, ConfigError> {
    // Check for null bytes and other invalid characters
    if path.contains('\0') {
        return Err(ConfigError::InvalidConfig(
            "Path contains invalid characters".to_string(),
        ));
    }

    let path = shellexpand::tilde(path);
    let path = PathBuf::from(path.as_ref());

    // Check if parent directory exists
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            return Err(ConfigError::InvalidConfig(format!(
                "Parent directory does not exist: {}",
                parent.display()
            )));
        }

        // Check if directory is writable
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if let Ok(metadata) = parent.metadata() {
                if metadata.mode() & 0o200 == 0 {
                    return Err(ConfigError::InvalidConfig(format!(
                        "Directory is not writable: {}",
                        parent.display()
                    )));
                }
            }
        }
    }

    // Additional path validation
    if path.as_os_str().is_empty() {
        return Err(ConfigError::InvalidConfig(
            "Path cannot be empty".to_string(),
        ));
    }

    Ok(path)
}

fn validate_storage_type(value: &str) -> Result<(), ConfigError> {
    if !VALID_STORAGE_TYPES.contains(&value) {
        return Err(ConfigError::InvalidConfig(format!(
            "storage.type must be one of: {}",
            VALID_STORAGE_TYPES.join(", ")
        )));
    }
    Ok(())
}

fn validate_priority(value: &str) -> Result<(), ConfigError> {
    if !VALID_PRIORITIES.contains(&value) {
        return Err(ConfigError::InvalidConfig(format!(
            "priority must be one of: {}",
            VALID_PRIORITIES.join(", ")
        )));
    }
    Ok(())
}

fn validate_lifespan(value: &str) -> Result<u32, ConfigError> {
    value.parse().map_err(|_| {
        ConfigError::InvalidConfig(
            "deleted-task-lifespan must be a positive integer or 0".to_string(),
        )
    })
}

// A `Config` value is used to represent two conceptually different things,
// and keeping them straight is the whole point of this module:
//
// - "stored" config: what is actually written in `trtodo-config.json`. A
//   `None` field here means *unset* - the user (or a fresh install) never
//   wrote anything for it. This is what `ConfigStorage::load()` returns and
//   what `config list`'s `*` markers are computed from.
// - "effective" config: the stored value if present, otherwise the
//   documented default from the README's configuration table. This is what
//   callers that actually need a usable value (`ConfigManager::get`, and
//   through it `open_storage`) should read.
//
// `#[serde(default)]` (rather than a `default = "..."` helper) makes an
// absent key deserialize to `None`, i.e. "unset" - it does NOT fill in the
// documented default. `skip_serializing_if` is the other half: without it,
// serializing a `None` field writes an explicit JSON `null`, and on the
// *next* load that key is present, so serde uses the `null` instead of ever
// consulting a default. Together they let "never written" and "written as
// null" collapse into the same, correct "unset" state on every read.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_task_lifespan: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_priority: Option<String>,
    /// The first-run marker: `Some(true)` records that the
    /// offer to create the default "Home"/"Work" categories has already been
    /// resolved (accepted, declined, or found unnecessary because categories
    /// already existed), so it is never made a second time.
    ///
    /// This is bookkeeping, not a preference, so it is deliberately absent
    /// from `ConfigManager::{get, set, unset, list}` and from the README's
    /// configuration table: `config list` stays a faithful rendering of that
    /// table, and there is no user-facing key whose meaning we would have to
    /// define. It is still plain JSON in the same file, so anyone who wants
    /// the offer back can delete the line (or set it to `false`, which reads
    /// as "not yet offered" - see `Config::default_categories_offered`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_categories_offered: Option<bool>,
}

impl Config {
    /// A config where nothing has ever been stored: every field is `None`.
    ///
    /// This is the honest representation for "no config file exists yet" /
    /// "the config file is empty" (see `ConfigStorage::load`), and for the
    /// vestigial `config` field embedded in the task-data file
    /// (`StorageData`), which isn't the real config store and should never
    /// be populated with resolved defaults.
    ///
    /// Deliberately distinct from `Config::default()`: collapsing "unset"
    /// and "documented default" into one value is a bug that has bitten
    /// this code before (it makes `config list` unable to tell "you set
    /// this" from "this is a fallback").
    pub fn unset() -> Self {
        Self {
            deleted_task_lifespan: None,
            storage_type: None,
            storage_path: None,
            default_category: None,
            default_priority: None,
            default_categories_offered: None,
        }
    }

    /// Whether the first-run offer of the default "Home"/"Work" categories
    /// has already been resolved and must not be made again.
    ///
    /// Anything other than a stored `true` - the key absent (a genuinely
    /// fresh install), or an explicit `false` - means "not yet offered".
    pub fn default_categories_offered(&self) -> bool {
        self.default_categories_offered.unwrap_or(false)
    }

    /// Resolves every unset field to its documented default, leaving any
    /// explicitly-stored value untouched. Used by `ConfigManager::get` (and
    /// transitively `main::open_storage`), which need a usable value rather
    /// than a raw, possibly-absent one.
    fn with_effective_defaults(&self) -> Self {
        let defaults = Self::default();
        Self {
            deleted_task_lifespan: self
                .deleted_task_lifespan
                .or(defaults.deleted_task_lifespan),
            storage_type: self.storage_type.clone().or(defaults.storage_type),
            storage_path: self.storage_path.clone().or(defaults.storage_path),
            default_category: self.default_category.clone().or(defaults.default_category),
            default_priority: self.default_priority.clone().or(defaults.default_priority),
            // Not a documented setting with a default value - "unset" is the
            // whole signal here, so it is carried through untouched.
            default_categories_offered: self.default_categories_offered,
        }
    }

    fn validate(&self) -> Result<(), ConfigError> {
        if let Some(ref storage_type) = self.storage_type {
            validate_storage_type(storage_type)?;
        }
        if let Some(ref priority) = self.default_priority {
            validate_priority(priority)?;
        }
        if let Some(ref path) = self.storage_path {
            validate_storage_path(path)?;
        }
        Ok(())
    }
}

/// The single source of truth for the documented defaults from the README's
/// configuration table. `with_effective_defaults`, `ConfigManager::get`, and
/// `ConfigManager::list`'s fallback values all go through this - none of
/// them hardcode a default value of their own.
impl Default for Config {
    fn default() -> Self {
        Self {
            deleted_task_lifespan: default_deleted_task_lifespan(),
            storage_type: default_storage_type(),
            storage_path: default_storage_path(),
            // `default-category` has no documented default (README: `null`).
            default_category: None,
            default_priority: default_priority(),
            // Not a user-facing setting: an unset marker means "the first-run
            // offer hasn't been made yet", which is exactly right for a
            // fresh install.
            default_categories_offered: None,
        }
    }
}

fn default_deleted_task_lifespan() -> Option<u32> {
    Some(0)
}

fn default_storage_type() -> Option<String> {
    Some("json".to_string())
}

/// Degrades to `None` (rather than panicking) when the home directory can't
/// be determined, e.g. in a test/CI environment with `$HOME` pointed at a
/// nonexistent path. `Config::default()` is now reachable from many more
/// places than before this fix, so a panic here would be much easier to hit.
fn default_storage_path() -> Option<String> {
    dirs::home_dir().map(|home| {
        home.join(".config")
            .join("trtodo")
            .to_string_lossy()
            .to_string()
    })
}

fn default_priority() -> Option<String> {
    Some("medium".to_string())
}

pub struct ConfigManager {
    storage: Box<dyn Storage>,
}

impl ConfigManager {
    pub fn new(config_path: Option<&Path>) -> Result<Self, ConfigError> {
        let config_path = if let Some(path) = config_path {
            path.to_path_buf()
        } else {
            let home = dirs::home_dir().expect("Could not determine home directory");
            home.join(".config")
                .join("trtodo")
                .join("trtodo-config.json")
        };

        let storage =
            ConfigStorage::new(&config_path).map_err(|e| ConfigError::Storage(e.to_string()))?;
        let storage = Box::new(storage);

        Ok(Self { storage })
    }

    /// Returns the *effective* value for `key`: whatever is stored, or the
    /// documented default if nothing was ever set. This is what callers that
    /// need a usable value (like `main::open_storage`, which picks the
    /// storage backend and path) should call.
    pub fn get(&self, key: &str) -> Option<String> {
        let config = self.effective_config();
        match key {
            "deleted-task-lifespan" => config.deleted_task_lifespan.map(|v| v.to_string()),
            "storage.type" => config.storage_type.map(|v| v.to_string()),
            "storage.path" => config.storage_path.map(|v| v.to_string()),
            "default-category" => config.default_category.clone(),
            "default-priority" => config.default_priority.map(|v| v.to_string()),
            _ => None,
        }
    }

    /// Whether the first-run offer of the default "Home"/"Work" categories
    /// has already been resolved.
    ///
    /// Reads the *stored* config, never the effective one: the point of the
    /// marker is to distinguish "nothing has ever been written here" from
    /// "the user answered", and resolving it through defaults would erase
    /// exactly that distinction.
    pub fn default_categories_offered(&self) -> bool {
        self.stored_config().default_categories_offered()
    }

    /// Records that the first-run offer has been resolved, so no later run
    /// makes it again - including after the user deliberately deletes every
    /// category, which is a legitimate empty state and not a fresh install.
    ///
    /// Deliberately not reachable through `set`: this is bookkeeping rather
    /// than a documented configuration key (see `Config`).
    pub fn record_default_categories_offer(&mut self) -> Result<(), ConfigError> {
        // Load-mutate-save, like `set`, so any keys the user has already
        // stored survive.
        let mut data = self
            .storage
            .load()
            .map_err(|e| ConfigError::Storage(e.to_string()))?;
        data.config.default_categories_offered = Some(true);
        self.storage
            .save(&data)
            .map_err(|e| ConfigError::Storage(e.to_string()))
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<(), ConfigError> {
        let mut data = self
            .storage
            .load()
            .map_err(|e| ConfigError::Storage(e.to_string()))?;
        let mut config = data.config.clone();

        match key {
            "deleted-task-lifespan" => {
                let value = validate_lifespan(value)?;
                config.deleted_task_lifespan = Some(value);
            }
            // Note what is deliberately *not* here: this used to also print
            // "Warning: Changing storage type may require data migration" on
            // every single `set`, and stash the outgoing value in an
            // `old_storage_type` field feeding `needs_migration()` /
            // `get_migration_info()` that nothing ever called. The warning
            // named a migration that did not exist, fired even when there was
            // no data to migrate, and told the user nothing they could act
            // on. Moving the data is now a real, tested step
            // (`main::carry_data_across_backend_switch`), and it runs *before*
            // this method so a failure leaves the setting - and therefore the
            // user's view of their data - unchanged. Anything it needs to say
            // about the switch, it says itself and accurately.
            "storage.type" => {
                validate_storage_type(value)?;
                config.storage_type = Some(value.to_string());
            }
            "storage.path" => {
                let path = validate_storage_path(value)?;
                config.storage_path = Some(path.to_string_lossy().to_string());
            }
            "default-category" => {
                // Still stored without checking that the category exists, and
                // deliberately so. `ConfigManager` owns the *config* store
                // only; the categories live in the task store, which is opened
                // from `storage.type`/`storage.path` - config values this very
                // method may be in the middle of changing. Validating here
                // would mean opening (and creating) task storage as a side
                // effect of `config set`, which is the one command that today
                // never touches it (see `main::open_storage`).
                //
                // Validation would also be worth less than it looks:
                // categories can be deleted after the fact, so a check here
                // could never make the value trustworthy at the point of use.
                // `main::resolve_add_category` therefore resolves it - and
                // reports it as an error - when `add` actually falls through
                // to it.
                config.default_category = Some(value.to_string());
            }
            "default-priority" => {
                validate_priority(value)?;
                config.default_priority = Some(value.to_string());
            }
            _ => {
                return Err(ConfigError::InvalidKey(key.to_string()));
            }
        }
        config.validate()?;
        data.config = config;
        self.storage
            .save(&data)
            .map_err(|e| ConfigError::Storage(e.to_string()))?;
        Ok(())
    }

    pub fn unset(&mut self, key: &str) -> Result<(), ConfigError> {
        // Operate on the *stored* config, not the effective one - clearing a
        // field should make it unset (and therefore fall back to the
        // documented default again), not re-write whatever the effective
        // value happened to be.
        let mut config = self.stored_config();
        match key {
            "deleted-task-lifespan" => config.deleted_task_lifespan = None,
            "storage.type" => config.storage_type = None,
            "storage.path" => config.storage_path = None,
            "default-category" => config.default_category = None,
            "default-priority" => config.default_priority = None,
            _ => return Err(ConfigError::InvalidKey(key.to_string())),
        }
        let mut data = self.storage.load().unwrap();
        data.config = config;
        self.storage.save(&data)?;
        Ok(())
    }

    pub fn list(&self) -> Vec<(String, String, bool)> {
        // "Is this a default" comes from the *stored* config (was the field
        // ever set?); the printed value comes from the *effective* config
        // (resolved through the same documented defaults as `get`). Neither
        // is hardcoded here - `default-category` legitimately prints `null`
        // simply because it has no `default_*()` helper to fall back to.
        let stored = self.stored_config();
        let effective = stored.with_effective_defaults();
        vec![
            (
                "deleted-task-lifespan".to_string(),
                effective
                    .deleted_task_lifespan
                    .map_or_else(|| "null".to_string(), |v| v.to_string()),
                stored.deleted_task_lifespan.is_none(),
            ),
            (
                "storage.type".to_string(),
                effective
                    .storage_type
                    .clone()
                    .unwrap_or_else(|| "null".to_string()),
                stored.storage_type.is_none(),
            ),
            (
                "storage.path".to_string(),
                effective
                    .storage_path
                    .clone()
                    .unwrap_or_else(|| "null".to_string()),
                stored.storage_path.is_none(),
            ),
            (
                "default-category".to_string(),
                effective
                    .default_category
                    .clone()
                    .unwrap_or_else(|| "null".to_string()),
                stored.default_category.is_none(),
            ),
            (
                "default-priority".to_string(),
                effective
                    .default_priority
                    .clone()
                    .unwrap_or_else(|| "null".to_string()),
                stored.default_priority.is_none(),
            ),
        ]
    }

    /// The raw, on-disk config: `None` fields are genuinely unset.
    fn stored_config(&self) -> Config {
        self.storage.load().unwrap().config
    }

    /// The stored config with unset fields resolved to their documented
    /// defaults.
    fn effective_config(&self) -> Config {
        self.stored_config().with_effective_defaults()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::{NamedTempFile, TempDir};

    #[test]
    fn test_config_manager() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");
        let mut manager = ConfigManager::new(Some(config_path.as_path())).unwrap();

        // Test setting storage type
        assert!(manager.set("storage.type", "json").is_ok());
        assert_eq!(manager.get("storage.type"), Some("json".to_string()));

        // Test setting storage path
        let storage_path = "~/.config/trtodo";
        assert!(manager.set("storage.path", storage_path).is_ok());
        assert_eq!(
            manager.get("storage.path"),
            Some(shellexpand::tilde(storage_path).to_string())
        );

        // Test setting default category
        assert!(manager.set("default-category", "work").is_ok());
        assert_eq!(manager.get("default-category"), Some("work".to_string()));

        // Test setting default priority
        assert!(manager.set("default-priority", "high").is_ok());
        assert_eq!(manager.get("default-priority"), Some("high".to_string()));

        // Test setting deleted task lifespan
        assert!(manager.set("deleted-task-lifespan", "7").is_ok());
        assert_eq!(manager.get("deleted-task-lifespan"), Some("7".to_string()));

        // Test unsetting values
        assert!(manager.unset("default-category").is_ok());
        assert_eq!(manager.get("default-category"), None);
    }

    // Previously asserted `None` for every key on a fresh config file - that
    // encoded the bug where documented defaults were never applied.
    // `ConfigManager::get` returns the *effective* value, so a fresh install
    // must report the README's documented defaults, not `None`.
    // `default-category` has no documented default, so `None` there is
    // still correct.
    #[test]
    fn test_config_manager_defaults() {
        let temp_file = NamedTempFile::new().unwrap();
        let manager = ConfigManager::new(Some(temp_file.path())).unwrap();

        assert_eq!(manager.get("deleted-task-lifespan"), Some("0".to_string()));
        assert_eq!(manager.get("storage.type"), Some("json".to_string()));
        assert_eq!(manager.get("default-category"), None);
        assert_eq!(manager.get("default-priority"), Some("medium".to_string()));
    }

    // Previously only asserted that every key reports as a default (still
    // true - nothing has been set), but not *what* the printed value is,
    // which is exactly what the earlier implementation got wrong (the value
    // was `null` instead of the documented default).
    #[test]
    fn test_config_manager_list() {
        let temp_file = NamedTempFile::new().unwrap();
        let manager = ConfigManager::new(Some(temp_file.path())).unwrap();

        let list = manager.list();
        assert_eq!(list.len(), 5);
        assert!(list.iter().all(|(_, _, is_default)| *is_default));

        let value_of = |key: &str| list.iter().find(|(k, _, _)| k == key).unwrap().1.clone();
        assert_eq!(value_of("deleted-task-lifespan"), "0");
        assert_eq!(value_of("storage.type"), "json");
        assert_eq!(value_of("default-category"), "null");
        assert_eq!(value_of("default-priority"), "medium");
    }

    /// Regression test for the second, easier-to-miss half of that bug: once
    /// *any* key has been `set`, the config file exists and every other key
    /// must still report as a default on the next load - not as an explicit
    /// `null` that beats the `#[serde(default)]` helper. A bare `impl
    /// Default for Config` (without `skip_serializing_if`) passes
    /// `test_config_manager_defaults` above but fails this one.
    #[test]
    fn test_untouched_keys_stay_default_after_a_set_and_reload() {
        let temp_file = NamedTempFile::new().unwrap();
        let config_path = temp_file.path().to_path_buf();

        let mut manager = ConfigManager::new(Some(&config_path)).unwrap();
        manager.set("default-priority", "high").unwrap();

        // Simulate the next process invocation (e.g. `config list` running
        // after a `config set`) by loading a fresh `ConfigManager`.
        let manager = ConfigManager::new(Some(&config_path)).unwrap();
        let list = manager.list();
        let entry = |key: &str| list.iter().find(|(k, _, _)| k == key).unwrap().clone();

        let (_, value, is_default) = entry("default-priority");
        assert_eq!(value, "high");
        assert!(!is_default);

        for key in [
            "deleted-task-lifespan",
            "storage.type",
            "storage.path",
            "default-category",
        ] {
            let (_, _, is_default) = entry(key);
            assert!(is_default, "{key} should still be a default");
        }
        assert_eq!(entry("deleted-task-lifespan").1, "0");
        assert_eq!(entry("storage.type").1, "json");
    }

    /// `Config::default()` (the documented, *effective* defaults) and
    /// `Config::unset()` (nothing stored yet) must never collapse into the
    /// same value - doing so is exactly the bug described on `unset`.
    #[test]
    fn test_default_and_unset_are_distinct() {
        let defaults = Config::default();
        assert_eq!(defaults.deleted_task_lifespan, Some(0));
        assert_eq!(defaults.storage_type, Some("json".to_string()));
        assert_eq!(defaults.default_priority, Some("medium".to_string()));
        assert_eq!(defaults.default_category, None); // no documented default

        let unset = Config::unset();
        assert_eq!(unset.deleted_task_lifespan, None);
        assert_eq!(unset.storage_type, None);
        assert_eq!(unset.storage_path, None);
        assert_eq!(unset.default_category, None);
        assert_eq!(unset.default_priority, None);
    }

    /// A fresh config (nothing stored) must report the first-run
    /// offer as *not* made, and recording it must survive into the next
    /// process invocation - that persistence is the whole mechanism that
    /// stops the offer being repeated.
    #[test]
    fn test_default_categories_offer_marker_persists() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");

        let mut manager = ConfigManager::new(Some(&config_path)).unwrap();
        assert!(!manager.default_categories_offered());
        assert!(
            !config_path.exists(),
            "merely asking must not create the config file"
        );

        manager.record_default_categories_offer().unwrap();
        assert!(manager.default_categories_offered());

        let manager = ConfigManager::new(Some(&config_path)).unwrap();
        assert!(manager.default_categories_offered());
    }

    /// The marker is bookkeeping, not a documented setting: it must not
    /// appear in `config list` (which mirrors the README's configuration
    /// table) and must not be reachable through `set` / `default`.
    #[test]
    fn test_default_categories_offer_marker_is_not_a_user_facing_key() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");

        let mut manager = ConfigManager::new(Some(&config_path)).unwrap();
        manager.record_default_categories_offer().unwrap();

        let list = manager.list();
        assert_eq!(list.len(), 5);
        assert!(!list
            .iter()
            .any(|(key, _, _)| key.contains("default-categories")));
        assert_eq!(manager.get("default-categories-offered"), None);
        assert!(manager.set("default-categories-offered", "false").is_err());
        assert!(manager.unset("default-categories-offered").is_err());
    }

    /// Recording the marker must not disturb settings the user already
    /// stored, and setting a key afterwards must not wipe the marker - both
    /// halves of the load-mutate-save round trip.
    #[test]
    fn test_recording_the_marker_preserves_other_keys_and_vice_versa() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.json");

        let mut manager = ConfigManager::new(Some(&config_path)).unwrap();
        manager.set("default-priority", "high").unwrap();
        manager.record_default_categories_offer().unwrap();
        assert_eq!(manager.get("default-priority"), Some("high".to_string()));

        manager.set("deleted-task-lifespan", "7").unwrap();
        assert!(manager.default_categories_offered());

        let manager = ConfigManager::new(Some(&config_path)).unwrap();
        assert!(manager.default_categories_offered());
        assert_eq!(manager.get("default-priority"), Some("high".to_string()));
        assert_eq!(manager.get("deleted-task-lifespan"), Some("7".to_string()));
    }
}
