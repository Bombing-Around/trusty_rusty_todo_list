//! End-to-end coverage of `trtodo deleted flush` and the automatic purge
//! driven by `deleted-task-lifespan` (issue #6).
//!
//! Every invocation is pointed at a `--config` file inside a `TempDir`, and
//! that config points `storage.path` at the same `TempDir`, so the real
//! `~/.config/trtodo` and the developer's actual todo data are never read or
//! written. This mirrors `tests/task_commands.rs`/`tests/category_commands.rs`'s
//! harness exactly - see `home_guard` below, which is the load-bearing part:
//! a previous PR (#15) was rejected specifically because its test suite wiped
//! the developer's real todo data, and this issue is about destroying data,
//! so the guard is asserted on explicitly rather than just relied upon.

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
    /// instead of touching the developer's real config/todo data.
    fn home_guard(&self) -> &Path {
        Path::new("/nonexistent-trtodo-test-home")
    }

    /// The exact path `open_storage` would create task data under if it ever
    /// fell back to `$HOME` instead of the configured `storage.path`
    /// (`main::open_storage`: `$HOME/.config/trtodo`). Asserting on this
    /// precise, app-owned subpath - rather than on `home_guard()` itself -
    /// is deliberate: some environments (e.g. a CI sandbox) may already have
    /// unrelated content sitting at the guard root for reasons outside this
    /// codebase's control, which would make "the guard root doesn't exist"
    /// a false failure. What actually matters, and what this proves, is
    /// that trtodo itself never wrote anything there.
    fn app_home_marker(&self) -> std::path::PathBuf {
        self.home_guard().join(".config").join("trtodo")
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
}

#[test]
fn flush_permanently_removes_a_deleted_task() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);
    trtodo.ok(&["delete", "Buy milk", "--category", "Work"]);

    // Soft-deleted: already gone from `list`, but flush hasn't run yet.
    let out = trtodo.ok(&["list"]);
    assert!(!out.contains("Buy milk"), "{out}");

    let out = trtodo.ok(&["deleted", "flush"]);
    assert!(out.contains("Flushed 1 deleted task(s)"), "{out}");

    // A second flush finds nothing left to purge.
    let out = trtodo.ok(&["deleted", "flush"]);
    assert!(out.contains("Flushed 0 deleted task(s)"), "{out}");

    // trtodo must never have created its own data under the guarded $HOME.
    assert!(
        !trtodo.app_home_marker().exists(),
        "flush must never touch the guarded $HOME path"
    );
}

#[test]
fn flush_with_nothing_deleted_is_a_clean_no_op() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);

    let out = trtodo.ok(&["deleted", "flush"]);
    assert!(out.contains("Flushed 0 deleted task(s)"), "{out}");

    // The live task is completely unaffected.
    let out = trtodo.ok(&["list"]);
    assert!(out.contains("Buy milk"), "{out}");

    assert!(!trtodo.app_home_marker().exists());
}

#[test]
fn flush_does_not_touch_live_tasks() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);
    trtodo.ok(&["add", "Walk dog", "--category", "Work"]);
    trtodo.ok(&["delete", "Buy milk", "--category", "Work"]);

    trtodo.ok(&["deleted", "flush"]);

    let out = trtodo.ok(&["list"]);
    assert!(out.contains("Walk dog"), "{out}");
    assert!(!out.contains("Buy milk"), "{out}");

    // The still-live task is still reachable and operable by name.
    trtodo.ok(&["check", "Walk dog", "--category", "Work"]);
    let out = trtodo.ok(&["list", "--completed"]);
    assert!(out.contains("Walk dog"), "{out}");

    assert!(!trtodo.app_home_marker().exists());
}

#[test]
fn default_lifespan_of_zero_never_auto_purges() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);
    trtodo.ok(&["delete", "Buy milk", "--category", "Work"]);

    // `deleted-task-lifespan` defaults to 0 ("never"); the deleted task's
    // `deleted_at` isn't backdated here (there's no CLI knob to fake the
    // clock), so this simply confirms that plain invocations - which each
    // run the automatic-purge sweep before dispatching - never destroy a
    // soft-deleted task under the default configuration. The age-gated
    // purge logic itself (with a backdated `deleted_at`) is covered at the
    // storage/manager layer in src/storage/mod.rs and src/task_manager.rs,
    // since faking elapsed days end-to-end through the CLI isn't practical.
    trtodo.ok(&["config", "list"]);
    trtodo.ok(&["list"]);
    trtodo.ok(&["category", "list"]);

    // The task is still physically present in storage - if the automatic
    // sweep had wrongly purged it under the default threshold, this flush
    // would report 0 instead of 1.
    let out = trtodo.ok(&["deleted", "flush"]);
    assert!(out.contains("Flushed 1 deleted task(s)"), "{out}");

    assert!(!trtodo.app_home_marker().exists());
}
