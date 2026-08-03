//! End-to-end coverage of `trtodo config set storage.type=...`.
//!
//! Once the original "file is not a database" panic was gone, this was what
//! remained: each backend keeps its own data file, so changing
//! `storage.type` made every category and task vanish from view. Nothing was
//! destroyed - the old file sat there untouched and switching back revealed it
//! again - but the only explanation offered was "Warning: Changing storage
//! type may require data migration", naming a migration that did not exist.
//!
//! These tests pin the three outcomes a switch can now have: carry the data
//! across, refuse to clobber a destination that already has data (and say so),
//! or stay quiet because there was nothing to carry.
//!
//! Every invocation is pointed at a `--config` file inside a `TempDir` with
//! `storage.path` pointing into the same `TempDir`, and `$HOME` at a path that
//! does not exist, so the real `~/.config/trtodo` is never read or written.

use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

struct Trtodo {
    _dir: TempDir,
    config_path: PathBuf,
    data_dir: PathBuf,
}

impl Trtodo {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("trtodo-config.json");
        let data_dir = dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        let this = Self {
            config_path,
            data_dir,
            _dir: dir,
        };
        this.ok(&[
            "config",
            "set",
            &format!("storage.path={}", this.data_dir.display()),
        ]);
        this
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_trusty_rusty_todo_list"))
            .arg("--config")
            .arg(&self.config_path)
            // Make sure nothing can silently fall back to a real home directory.
            .env("HOME", Path::new("/nonexistent-trtodo-test-home"))
            .args(args)
            .output()
            .expect("failed to run trtodo")
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

    /// Both streams of a successful command: the switch reports a migration on
    /// stdout but warns about a refused one on stderr, and tests here care
    /// about each independently.
    fn ok_both(&self, args: &[&str]) -> (String, String) {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "command {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        (
            String::from_utf8(output.stdout).unwrap(),
            String::from_utf8(output.stderr).unwrap(),
        )
    }

    fn json_file(&self) -> PathBuf {
        self.data_dir.join("trtodo-data.json")
    }

    fn sqlite_file(&self) -> PathBuf {
        self.data_dir.join("trtodo-data.db")
    }

    /// Seeds a category, a task in it, and a `category use` context - the
    /// smallest set of state that would be visibly "lost" by a switch.
    fn seed(&self) {
        self.ok(&["category", "add", "Work"]);
        self.ok(&["add", "--category", "Work", "Finish report"]);
        self.ok(&["category", "use", "Work"]);
    }
}

/// The headline fix: switching from JSON to SQLite carries the user's data
/// across instead of stranding it, and says so plainly.
#[test]
fn switching_backend_carries_existing_data_across() {
    let trtodo = Trtodo::new();
    trtodo.seed();

    let json_before = std::fs::read_to_string(trtodo.json_file()).unwrap();
    assert!(json_before.contains("Finish report"));

    let (stdout, stderr) = trtodo.ok_both(&["config", "set", "storage.type=sqlite"]);
    assert!(
        stdout.contains("Migrated 1 task(s) and 1 category from json to sqlite"),
        "expected an accurate migration report, got stdout: {stdout}"
    );
    assert!(
        stdout.contains(&trtodo.json_file().display().to_string()),
        "the user should be told exactly where their previous data still lives, got: {stdout}"
    );
    // The old "may require data migration" hedge is gone: a migration either
    // happened or it didn't, and this one did.
    assert!(
        !stderr.contains("may require data migration"),
        "the vague unconditional warning should be gone, got stderr: {stderr}"
    );

    // The data is genuinely there under the new backend...
    let categories = trtodo.ok(&["category", "list"]);
    assert!(categories.contains("Work"), "{categories}");
    let tasks = trtodo.ok(&["list"]);
    assert!(tasks.contains("Finish report"), "{tasks}");
    // ... including the `category use` context.
    assert!(
        categories.contains("Work (current)"),
        "the category context should survive the switch, got: {categories}"
    );

    // ... and the switch was non-destructive: the JSON file is byte-for-byte
    // what it was.
    assert_eq!(
        std::fs::read_to_string(trtodo.json_file()).unwrap(),
        json_before,
        "migrating must never rewrite or truncate the source store"
    );
    assert!(trtodo.sqlite_file().exists());
}

/// Switching back to a backend that already holds data must refuse to
/// overwrite it, and must tell the user where the *other* set of data is and
/// how to get back to it. Merging the two would mean renumbering IDs, which is
/// a different feature with different risks.
#[test]
fn switching_back_to_a_populated_backend_refuses_to_clobber_and_says_so() {
    let trtodo = Trtodo::new();
    trtodo.seed();
    trtodo.ok(&["config", "set", "storage.type=sqlite"]);

    // Diverge the two stores so a clobber in either direction is detectable.
    trtodo.ok(&["add", "--category", "Work", "Only in sqlite"]);

    let json_before = std::fs::read_to_string(trtodo.json_file()).unwrap();
    let sqlite_before = std::fs::read(trtodo.sqlite_file()).unwrap();

    let (_, stderr) = trtodo.ok_both(&["config", "set", "storage.type=json"]);
    assert!(
        stderr.contains("json storage already holds"),
        "the user should be told why nothing was migrated, got: {stderr}"
    );
    assert!(
        stderr.contains("nothing was overwritten"),
        "the user should be told their data is safe, got: {stderr}"
    );
    assert!(
        stderr.contains(&trtodo.sqlite_file().display().to_string())
            && stderr.contains("storage.type=sqlite"),
        "the user should be told where the other data is and how to reach it, got: {stderr}"
    );

    // Neither store was touched.
    assert_eq!(
        std::fs::read_to_string(trtodo.json_file()).unwrap(),
        json_before
    );
    assert_eq!(std::fs::read(trtodo.sqlite_file()).unwrap(), sqlite_before);

    // The advertised escape hatch really works: the sqlite-only task is still
    // reachable by switching back.
    let json_tasks = trtodo.ok(&["list"]);
    assert!(json_tasks.contains("Finish report"), "{json_tasks}");
    assert!(!json_tasks.contains("Only in sqlite"), "{json_tasks}");

    trtodo.ok(&["config", "set", "storage.type=sqlite"]);
    let sqlite_tasks = trtodo.ok(&["list"]);
    assert!(sqlite_tasks.contains("Only in sqlite"), "{sqlite_tasks}");
}

/// A first-ever switch, with nothing stored yet, must be completely quiet -
/// the old warning fired here too, worrying users about a migration of no
/// data at all.
#[test]
fn switching_with_nothing_stored_is_quiet() {
    let trtodo = Trtodo::new();

    let (stdout, stderr) = trtodo.ok_both(&["config", "set", "storage.type=sqlite"]);
    assert_eq!(stdout, "Configuration updated successfully\n", "{stdout}");
    assert_eq!(
        stderr, "",
        "a switch with no data to move should say nothing"
    );
}

/// Setting `storage.type` to the value it already has isn't a switch at all,
/// so it must not report a migration (and must not risk writing anything).
#[test]
fn setting_storage_type_to_its_current_value_is_a_no_op() {
    let trtodo = Trtodo::new();
    trtodo.seed();
    let json_before = std::fs::read_to_string(trtodo.json_file()).unwrap();

    let (stdout, stderr) = trtodo.ok_both(&["config", "set", "storage.type=json"]);
    assert_eq!(stdout, "Configuration updated successfully\n", "{stdout}");
    assert_eq!(stderr, "", "{stderr}");
    assert_eq!(
        std::fs::read_to_string(trtodo.json_file()).unwrap(),
        json_before
    );
    assert!(
        !trtodo.sqlite_file().exists(),
        "a no-op switch must not materialise the other backend's file"
    );
}

/// An unrecognised backend is still rejected by config validation, and the
/// migration hook must not get in the way of that message or leave a
/// half-created store behind.
#[test]
fn an_unknown_storage_type_is_still_rejected_cleanly() {
    let trtodo = Trtodo::new();
    trtodo.seed();

    let output = trtodo.run(&["config", "set", "storage.type=postgres"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("storage.type must be one of"),
        "expected the config validation error, got: {stderr}"
    );
    assert!(!trtodo.sqlite_file().exists());
    // The setting is unchanged, so the user is still looking at their data.
    assert!(trtodo.ok(&["list"]).contains("Finish report"));
}

/// A store that can't be read is exactly when a user most wants to switch away
/// from it. Blocking the switch would trap them on the broken backend - and
/// would do it by reporting an error about the backend they were leaving, not
/// the one they asked for. The switch must go through, with an honest note
/// that nothing was migrated and that the file was left alone.
#[test]
fn an_unreadable_source_store_warns_but_does_not_block_the_switch() {
    let trtodo = Trtodo::new();
    trtodo.seed();

    // Corrupt the JSON store behind the app's back.
    std::fs::write(trtodo.json_file(), "{ this is not valid json").unwrap();
    let corrupted = std::fs::read_to_string(trtodo.json_file()).unwrap();

    let (stdout, stderr) = trtodo.ok_both(&["config", "set", "storage.type=sqlite"]);
    assert!(
        stdout.contains("Configuration updated successfully"),
        "the switch itself must still succeed, got: {stdout}"
    );
    assert!(
        stderr.contains("could not read the existing json store")
            && stderr.contains("nothing was migrated"),
        "the user should be told why their data didn't come along, got: {stderr}"
    );

    // The unreadable file is left exactly as found - it is the only copy of
    // whatever is salvageable in there.
    assert_eq!(
        std::fs::read_to_string(trtodo.json_file()).unwrap(),
        corrupted
    );

    // And the new backend is usable, so the user isn't stuck.
    trtodo.ok(&["category", "add", "Fresh"]);
    assert!(trtodo.ok(&["category", "list"]).contains("Fresh"));
}
