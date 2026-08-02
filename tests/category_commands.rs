//! End-to-end coverage of `trtodo category ...`.
//!
//! Every invocation is pointed at a `--config` file inside a `TempDir`, and
//! that config points `storage.path` at the same `TempDir`, so the real
//! `~/.config/trtodo` is never read or written.

use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

struct Trtodo {
    _dir: TempDir,
    config_path: std::path::PathBuf,
}

impl Trtodo {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("trtodo-config.json");
        let this = Self {
            config_path,
            _dir: dir,
        };
        // Keep task data next to the config, inside the temp dir.
        let data_dir = this._dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();
        this.ok(&[
            "config",
            "set",
            &format!("storage.path={}", data_dir.display()),
        ]);
        this
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_trusty_rusty_todo_list"))
            .arg("--config")
            .arg(&self.config_path)
            // Make sure nothing can silently fall back to a real home directory.
            .env("HOME", self.home_guard())
            .args(args)
            .output()
            .expect("failed to run trtodo")
    }

    /// A path that does not exist: if any code path tries to use `$HOME`
    /// instead of the configured storage location, the test fails loudly
    /// instead of touching the developer's real config.
    fn home_guard(&self) -> &Path {
        Path::new("/nonexistent-trtodo-test-home")
    }

    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "command {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    fn fail(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            !output.status.success(),
            "command {:?} unexpectedly succeeded",
            args
        );
        String::from_utf8(output.stderr).unwrap()
    }
}

#[test]
fn category_lifecycle() {
    let trtodo = Trtodo::new();

    // Add
    let out = trtodo.ok(&["category", "add", "Work"]);
    assert!(out.contains("added with ID 1"), "{out}");
    trtodo.ok(&["category", "add", "Home"]);

    // List includes the magic Uncategorized category first
    let out = trtodo.ok(&["category", "list"]);
    assert!(out.contains("0: Uncategorized"), "{out}");
    assert!(out.contains("1: Work"), "{out}");
    assert!(out.contains("2: Home"), "{out}");

    // Duplicate names are rejected
    let err = trtodo.fail(&["category", "add", "work"]);
    assert!(err.contains("already exists"), "{err}");

    // Update
    trtodo.ok(&["category", "update", "Home", "Personal"]);
    let out = trtodo.ok(&["category", "list"]);
    assert!(out.contains("2: Personal"), "{out}");
    assert!(!out.contains("Home"), "{out}");

    // Delete frees the ID again (issue #16)
    trtodo.ok(&["category", "delete", "Work"]);
    let out = trtodo.ok(&["category", "add", "Errands"]);
    assert!(out.contains("added with ID 1"), "{out}");

    // Unknown categories are a clean error, not a panic
    let err = trtodo.fail(&["category", "delete", "Nope"]);
    assert!(err.starts_with("error: "), "{err}");
    assert!(err.contains("Nope"), "{err}");
}

#[test]
fn category_context_persists_between_runs() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);

    // No context yet
    let out = trtodo.ok(&["category", "show"]);
    assert!(out.contains("Uncategorized (ID: 0)"), "{out}");

    // Set context - and it survives into the *next* process invocation
    trtodo.ok(&["category", "use", "Work"]);
    let out = trtodo.ok(&["category", "show"]);
    assert!(out.contains("Current category: Work (ID: 1)"), "{out}");

    let out = trtodo.ok(&["category", "list"]);
    assert!(out.contains("1: Work (current)"), "{out}");

    // Categories can also be referenced by ID
    trtodo.ok(&["category", "clear"]);
    trtodo.ok(&["category", "use", "1"]);
    let out = trtodo.ok(&["category", "show"]);
    assert!(out.contains("Current category: Work (ID: 1)"), "{out}");

    // Clear
    trtodo.ok(&["category", "clear"]);
    let out = trtodo.ok(&["category", "show"]);
    assert!(out.contains("Uncategorized (ID: 0)"), "{out}");
}

#[test]
fn category_context_persists_with_sqlite_backend() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["config", "set", "storage.type=sqlite"]);

    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["category", "use", "Work"]);

    let out = trtodo.ok(&["category", "show"]);
    assert!(out.contains("Current category: Work (ID: 1)"), "{out}");
}
