//! End-to-end coverage of the `trtodo deleted` namespace - `list`,
//! `restore`, `flush` and its confirmation - plus the automatic purge
//! driven by `deleted-task-lifespan`.
//!
//! Note that every invocation here is by definition non-interactive: the
//! binary is spawned with `Command::output()`, so its stdin is not a
//! terminal and `StdinPrompter` returns `PromptError::NotInteractive` for
//! any prompt. That is not an obstacle to work around, it is the scripted
//! case these tests exist to pin down - `flush` must refuse without `--yes`,
//! and an ambiguous `restore` must fail cleanly rather than hang.
//!
//! Every invocation is pointed at a `--config` file inside a `TempDir`, and
//! that config points `storage.path` at the same `TempDir`, so the real
//! `~/.config/trtodo` and the developer's actual todo data are never read or
//! written. This mirrors `tests/task_commands.rs`/`tests/category_commands.rs`'s
//! harness exactly - see `home_guard` below, which is the load-bearing part:
//! an earlier attempt was rejected specifically because its test suite wiped
//! the developer's real todo data, and these commands are about destroying
//! data, so the guard is asserted on explicitly rather than just relied upon.

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

    /// Runs a command that is expected to fail, returning its stderr. The
    /// counterpart to `ok` for the paths that must refuse rather than act -
    /// `flush` with nobody to confirm at, an ambiguous `restore`, and so on.
    fn err(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(
            !output.status.success(),
            "command {:?} unexpectedly succeeded: {}",
            args,
            String::from_utf8_lossy(&output.stdout)
        );
        String::from_utf8(output.stderr).unwrap()
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

    let out = trtodo.ok(&["deleted", "flush", "--yes"]);
    assert!(out.contains("Flushed 1 deleted task(s)"), "{out}");

    // A second flush finds nothing left to purge.
    let out = trtodo.ok(&["deleted", "flush", "--yes"]);
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

    // Deliberately *without* `--yes`: with nothing at stake there is nothing
    // to confirm, so a flush of an empty deleted set stays a clean no-op
    // even with no terminal attached.
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

    trtodo.ok(&["deleted", "flush", "--yes"]);

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
    let out = trtodo.ok(&["deleted", "flush", "--yes"]);
    assert!(out.contains("Flushed 1 deleted task(s)"), "{out}");

    assert!(!trtodo.app_home_marker().exists());
}

#[test]
fn deleted_list_shows_id_title_category_and_deletion_date() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    let added = trtodo.ok(&["add", "Buy milk", "--category", "Work"]);
    trtodo.ok(&["delete", "Buy milk", "--category", "Work"]);

    // The ID the task was given, so the listing can be checked to report the
    // real one rather than a position in a list.
    let id = added
        .split("with ID ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .expect("add should report the new task's ID")
        .to_string();

    let out = trtodo.ok(&["deleted", "list"]);
    assert!(out.contains("Deleted tasks:"), "{out}");
    assert!(out.contains(&format!("{id}: Buy milk")), "{out}");
    // The *real* category, by name - soft deletion keeps `category_id`
    // intact, which is exactly what makes the restore below
    // lossless.
    assert!(out.contains("category: Work"), "{out}");
    // A deletion date in the documented format. Only the "deleted: <year>"
    // prefix is asserted on, so this doesn't turn into a clock test.
    assert!(out.contains("deleted: 20"), "{out}");

    // Still hidden from the ordinary listing and search, as before.
    let out = trtodo.ok(&["list"]);
    assert!(!out.contains("Buy milk"), "{out}");
    let out = trtodo.ok(&["list", "--search", "milk"]);
    assert!(!out.contains("Buy milk"), "{out}");

    assert!(!trtodo.app_home_marker().exists());
}

#[test]
fn deleted_list_says_so_explicitly_when_there_is_nothing_deleted() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);

    // "A flush would destroy nothing" has to be said out loud: silence here
    // is indistinguishable from a command that failed to look.
    let out = trtodo.ok(&["deleted", "list"]);
    assert!(out.contains("Deleted tasks:"), "{out}");
    assert!(out.contains("(none)"), "{out}");
    assert!(!out.contains("Buy milk"), "{out}");

    assert!(!trtodo.app_home_marker().exists());
}

#[test]
fn restore_returns_a_task_to_its_original_category() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);
    trtodo.ok(&["delete", "Buy milk", "--category", "Work"]);

    let out = trtodo.ok(&["deleted", "restore", "Buy milk"]);
    assert!(out.contains("Task 'Buy milk' restored"), "{out}");
    assert!(out.contains("Work"), "{out}");

    // Back in the listing, in the category it was deleted from.
    let out = trtodo.ok(&["list"]);
    assert!(out.contains("Buy milk"), "{out}");
    assert!(out.contains("category: Work"), "{out}");

    // And out of the deleted set, so a later flush can't destroy it.
    let out = trtodo.ok(&["deleted", "list"]);
    assert!(out.contains("(none)"), "{out}");
    let out = trtodo.ok(&["deleted", "flush"]);
    assert!(out.contains("Flushed 0 deleted task(s)"), "{out}");

    // A restored task is an ordinary task again: operable by name, and
    // re-deletable.
    trtodo.ok(&["check", "Buy milk", "--category", "Work"]);
    trtodo.ok(&["delete", "Buy milk", "--category", "Work"]);
    let out = trtodo.ok(&["deleted", "list"]);
    assert!(out.contains("Buy milk"), "{out}");

    assert!(!trtodo.app_home_marker().exists());
}

#[test]
fn restore_accepts_the_id_from_deleted_list() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    let added = trtodo.ok(&["add", "Buy milk", "--category", "Work"]);
    trtodo.ok(&["delete", "Buy milk", "--category", "Work"]);

    let id = added
        .split("with ID ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .expect("add should report the new task's ID")
        .to_string();

    let out = trtodo.ok(&["deleted", "restore", &id]);
    assert!(out.contains("Task 'Buy milk' restored"), "{out}");

    let out = trtodo.ok(&["list"]);
    assert!(out.contains("Buy milk"), "{out}");

    assert!(!trtodo.app_home_marker().exists());
}

#[test]
fn restore_refuses_a_live_task() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);

    // Restore searches only soft-deleted tasks - the inverse of every other
    // task command's scope - so a perfectly good live task is "not found"
    // here rather than being silently touched.
    let err = trtodo.err(&["deleted", "restore", "Buy milk"]);
    assert!(err.contains("no task matching 'Buy milk'"), "{err}");

    let out = trtodo.ok(&["list"]);
    assert!(out.contains("Buy milk"), "{out}");

    assert!(!trtodo.app_home_marker().exists());
}

#[test]
fn restore_of_an_ambiguous_title_fails_cleanly_with_no_terminal_attached() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["category", "add", "Home"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Home"]);
    trtodo.ok(&["delete", "Buy milk", "--category", "Work"]);
    trtodo.ok(&["delete", "Buy milk", "--category", "Home"]);

    // Two deleted tasks share the title, and there is no terminal to ask
    // which one. The README's disambiguation rule turns into a typed error
    // instead of a hang - and the advice is to use an ID, which
    // `deleted list` prints.
    let err = trtodo.err(&["deleted", "restore", "Buy milk"]);
    assert!(err.contains("no terminal is attached"), "{err}");

    // Neither task was restored, and both are still there to be rescued.
    let out = trtodo.ok(&["list"]);
    assert!(!out.contains("Buy milk"), "{out}");
    let out = trtodo.ok(&["deleted", "list"]);
    assert_eq!(out.matches("Buy milk").count(), 2, "{out}");

    assert!(!trtodo.app_home_marker().exists());
}

#[test]
fn flush_refuses_to_destroy_anything_without_confirmation() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);
    trtodo.ok(&["delete", "Buy milk", "--category", "Work"]);

    // Non-interactive (spawned with no terminal on stdin) and no `--yes`:
    // the deliberate choice is to refuse rather than destroy data
    // unattended, and to name the escape hatch in the error.
    let err = trtodo.err(&["deleted", "flush"]);
    assert!(err.contains("--yes"), "{err}");
    assert!(err.contains("deleted list"), "{err}");

    // Nothing was destroyed, so the task is still there to restore.
    let out = trtodo.ok(&["deleted", "list"]);
    assert!(out.contains("Buy milk"), "{out}");
    trtodo.ok(&["deleted", "restore", "Buy milk"]);
    let out = trtodo.ok(&["list"]);
    assert!(out.contains("Buy milk"), "{out}");

    assert!(!trtodo.app_home_marker().exists());
}

#[test]
fn flush_reports_which_tasks_it_removed() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);
    trtodo.ok(&["add", "Walk dog", "--category", "Work"]);
    trtodo.ok(&["delete", "Buy milk", "--category", "Work"]);
    trtodo.ok(&["delete", "Walk dog", "--category", "Work"]);

    // A count alone leaves the user with no way to find out what they lost -
    // the rows are gone by the time they could look.
    let out = trtodo.ok(&["deleted", "flush", "--yes"]);
    assert!(
        out.contains("The following 2 task(s) will be permanently removed:"),
        "{out}"
    );
    assert!(out.contains("Buy milk"), "{out}");
    assert!(out.contains("Walk dog"), "{out}");
    assert!(out.contains("category: Work"), "{out}");
    assert!(out.contains("Flushed 2 deleted task(s)"), "{out}");

    // Gone for good: nothing left to list or restore.
    let out = trtodo.ok(&["deleted", "list"]);
    assert!(out.contains("(none)"), "{out}");
    let err = trtodo.err(&["deleted", "restore", "Buy milk"]);
    assert!(err.contains("no task matching 'Buy milk'"), "{err}");

    assert!(!trtodo.app_home_marker().exists());
}

#[test]
fn flush_accepts_force_as_an_alias_for_yes() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);
    trtodo.ok(&["delete", "Buy milk", "--category", "Work"]);

    // `--force` and `-y` are the conventional spellings of the same escape
    // hatch; scripts shouldn't have to guess which one this CLI chose.
    let out = trtodo.ok(&["deleted", "flush", "--force"]);
    assert!(out.contains("Flushed 1 deleted task(s)"), "{out}");

    trtodo.ok(&["add", "Walk dog", "--category", "Work"]);
    trtodo.ok(&["delete", "Walk dog", "--category", "Work"]);
    let out = trtodo.ok(&["deleted", "flush", "-y"]);
    assert!(out.contains("Flushed 1 deleted task(s)"), "{out}");

    assert!(!trtodo.app_home_marker().exists());
}

#[test]
fn deleted_commands_survive_a_category_deletion() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);
    trtodo.ok(&["delete", "Buy milk", "--category", "Work"]);

    // Deleting the category reassigns every task in it - soft-deleted ones
    // included - to Uncategorized, so the restore below is still well
    // defined and lands somewhere real.
    trtodo.ok(&["category", "delete", "Work"]);

    let out = trtodo.ok(&["deleted", "list"]);
    assert!(out.contains("Buy milk"), "{out}");
    assert!(out.contains("category: Uncategorized"), "{out}");

    let out = trtodo.ok(&["deleted", "restore", "Buy milk"]);
    assert!(out.contains("Uncategorized"), "{out}");
    // Not the "original category no longer exists" wording: as far as the
    // task is concerned Uncategorized *is* its category by this point.
    assert!(!out.contains("no longer exists"), "{out}");

    let out = trtodo.ok(&["list"]);
    assert!(out.contains("Buy milk"), "{out}");
    assert!(out.contains("category: Uncategorized"), "{out}");

    assert!(!trtodo.app_home_marker().exists());
}
