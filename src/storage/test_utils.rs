//! Test-only helpers for exercising code against a real `Storage` backend.
//!
//! Everything here is gated behind `#[cfg(test)]`. The storage lives in a
//! `TempDir` that is removed when the `TestStorage` is dropped, so tests never
//! touch `$HOME` and never share a fixed relative path (which would make them
//! flaky under `cargo test`'s parallel execution).

use super::json::JsonStorage;
use super::Storage;
use crate::models::StorageData;
use tempfile::TempDir;

pub struct TestStorage {
    // Held purely for its `Drop`: it removes the directory backing `storage`.
    _temp_dir: TempDir,
    storage: JsonStorage,
}

impl TestStorage {
    pub fn new() -> Self {
        let temp_dir = tempfile::Builder::new()
            .prefix("trtodo_test")
            .tempdir()
            .expect("failed to create temporary directory");

        let storage = JsonStorage::new(temp_dir.path().join("test_storage.json"));
        storage
            .save(&StorageData::new())
            .expect("failed to initialize test storage");

        Self {
            _temp_dir: temp_dir,
            storage,
        }
    }

    pub fn storage(&self) -> &dyn Storage {
        &self.storage
    }
}
