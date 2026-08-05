//! End-to-end coverage of `trt category ...`.
//!
//! Every invocation is pointed at a `--config` file inside a `TempDir`, and
//! that config points `storage.path` at the same `TempDir`, so the real
//! `~/.config/trt` is never read or written.

use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

struct Trt {
    _dir: TempDir,
    config_path: std::path::PathBuf,
}

impl Trt {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("trt-config.json");
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
        Command::new(env!("CARGO_BIN_EXE_trt"))
            .arg("--config")
            .arg(&self.config_path)
            // Make sure nothing can silently fall back to a real home directory.
            .env("HOME", self.home_guard())
            .args(args)
            .output()
            .expect("failed to run trt")
    }

    /// A path that does not exist: if any code path tries to use `$HOME`
    /// instead of the configured storage location, the test fails loudly
    /// instead of touching the developer's real config.
    fn home_guard(&self) -> &Path {
        Path::new("/nonexistent-trt-test-home")
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
    let trt = Trt::new();

    // Add
    let out = trt.ok(&["category", "add", "Work"]);
    assert!(out.contains("added with ID 1"), "{out}");
    trt.ok(&["category", "add", "Home"]);

    // List includes the magic Uncategorized category first
    let out = trt.ok(&["category", "list"]);
    assert!(out.contains("0: Uncategorized (current)"), "{out}");
    assert!(out.contains("1: Work"), "{out}");
    assert!(out.contains("2: Home"), "{out}");

    // Duplicate names are rejected
    let err = trt.fail(&["category", "add", "work"]);
    assert!(err.contains("already exists"), "{err}");

    // Update
    trt.ok(&["category", "update", "Home", "Personal"]);
    let out = trt.ok(&["category", "list"]);
    assert!(out.contains("2: Personal"), "{out}");
    assert!(!out.contains("Home"), "{out}");

    // Delete frees the ID again
    trt.ok(&["category", "delete", "Work"]);
    let out = trt.ok(&["category", "add", "Errands"]);
    assert!(out.contains("added with ID 1"), "{out}");

    // Unknown categories are a clean error, not a panic
    let err = trt.fail(&["category", "delete", "Nope"]);
    assert!(err.starts_with("error: "), "{err}");
    assert!(err.contains("Nope"), "{err}");
}

#[test]
fn category_add_sets_a_description_and_list_shows_it() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work", "--description", "Job stuff"]);

    let out = trt.ok(&["category", "list"]);
    assert!(out.contains("Work (description: Job stuff)"), "{out}");

    // Omitting --description leaves it unset, and an unset description
    // contributes nothing to the line rather than a "(none)" placeholder -
    // `category list` is a scan, so a row with no description says nothing
    // about one.
    trt.ok(&["category", "add", "Home"]);
    let out = trt.ok(&["category", "list"]);
    assert!(
        out.lines().any(|line| line.trim_end() == "2: Home"),
        "{out}"
    );
}

#[test]
fn category_update_can_set_and_clear_a_description_independently_of_the_name() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);

    trt.ok(&["category", "update", "Work", "--description", "Job stuff"]);
    let out = trt.ok(&["category", "list"]);
    assert!(out.contains("Work (description: Job stuff)"), "{out}");

    // Renaming alone (no --description) leaves the description untouched.
    trt.ok(&["category", "update", "Work", "Werk"]);
    let out = trt.ok(&["category", "list"]);
    assert!(out.contains("Werk (description: Job stuff)"), "{out}");

    // Clearing back to empty requires the explicit flag, and leaves the line
    // carrying no description segment at all.
    trt.ok(&["category", "update", "Werk", "--clear-description"]);
    let out = trt.ok(&["category", "list"]);
    assert!(
        out.lines().any(|line| line.trim_end() == "1: Werk"),
        "{out}"
    );
}

#[test]
fn category_update_with_no_fields_at_all_is_a_clean_error() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);

    // Neither a new name, --description, nor --clear-description: nothing
    // to update, refused rather than silently printing a misleading
    // "updated" message.
    let err = trt.fail(&["category", "update", "Work"]);
    assert!(err.contains("nothing to update"), "{err}");
}

/// Backend parity for the description field: everything above runs against
/// the default JSON backend, this repeats the round trip against SQLite.
#[test]
fn category_description_round_trips_through_the_sqlite_backend() {
    let trt = Trt::new();
    trt.ok(&["config", "set", "storage.type=sqlite"]);
    trt.ok(&["category", "add", "Work", "--description", "Job stuff"]);

    let out = trt.ok(&["category", "list"]);
    assert!(out.contains("Work (description: Job stuff)"), "{out}");

    trt.ok(&["category", "update", "Work", "--clear-description"]);
    let out = trt.ok(&["category", "list"]);
    assert!(
        out.lines().any(|line| line.trim_end() == "1: Work"),
        "{out}"
    );
}

/// Renaming the category that `default-category` names must carry the
/// setting along, rather than leaving it pointed at a name that no longer
/// resolves to anything.
#[test]
fn category_update_carries_default_category_along() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&["config", "set", "default-category=Work"]);

    let out = trt.ok(&["category", "update", "Work", "Werk"]);
    assert!(
        out.contains("Updated config 'default-category' to 'Werk'"),
        "{out}"
    );

    // The setting now names the renamed category...
    let out = trt.ok(&["config", "list"]);
    assert!(out.contains("default-category = Werk"), "{out}");

    // ...and a bare `add` resolves it rather than erroring, landing the task
    // in the renamed category.
    let out = trt.ok(&["add", "Buy milk"]);
    assert!(out.contains("in category 'Werk'"), "{out}");

    let out = trt.ok(&["list"]);
    assert!(
        out.contains("Buy milk (priority: medium, category: Werk)"),
        "{out}"
    );
}

/// Renaming some *other* category must not disturb `default-category` -
/// only a rename of the category it actually names carries it along.
#[test]
fn category_update_leaves_unrelated_default_category_alone() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&["category", "add", "Home"]);
    trt.ok(&["config", "set", "default-category=Work"]);

    let out = trt.ok(&["category", "update", "Home", "Personal"]);
    assert!(!out.contains("default-category"), "{out}");

    let out = trt.ok(&["config", "list"]);
    assert!(out.contains("default-category = Work"), "{out}");
}

/// With `default-category` unset, a rename has nothing to carry along and
/// must not fabricate a value.
#[test]
fn category_update_with_no_default_category_set_is_a_no_op_for_config() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);

    let out = trt.ok(&["category", "update", "Work", "Werk"]);
    assert!(!out.contains("default-category"), "{out}");

    let out = trt.ok(&["config", "list"]);
    assert!(out.contains("*default-category = null"), "{out}");
}

/// The comparison between the stored `default-category` and the category
/// being renamed is case-insensitive, matching how category names are
/// matched everywhere else in this codebase (duplicate detection, `--category`
/// resolution).
#[test]
fn category_update_matches_default_category_case_insensitively() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&["config", "set", "default-category=work"]);

    let out = trt.ok(&["category", "update", "Work", "Werk"]);
    assert!(
        out.contains("Updated config 'default-category' to 'Werk'"),
        "{out}"
    );

    let out = trt.ok(&["config", "list"]);
    assert!(out.contains("default-category = Werk"), "{out}");
}

#[test]
fn category_context_persists_between_runs() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);

    // No context yet
    let out = trt.ok(&["category", "show"]);
    assert!(out.contains("Uncategorized (ID: 0)"), "{out}");

    // Set context - and it survives into the *next* process invocation
    trt.ok(&["category", "use", "Work"]);
    let out = trt.ok(&["category", "show"]);
    assert!(out.contains("Current category: Work (ID: 1)"), "{out}");

    let out = trt.ok(&["category", "list"]);
    assert!(out.contains("1: Work (current)"), "{out}");

    // Categories can also be referenced by ID
    trt.ok(&["category", "clear"]);
    trt.ok(&["category", "use", "1"]);
    let out = trt.ok(&["category", "show"]);
    assert!(out.contains("Current category: Work (ID: 1)"), "{out}");

    // Clear
    trt.ok(&["category", "clear"]);
    let out = trt.ok(&["category", "show"]);
    assert!(out.contains("Uncategorized (ID: 0)"), "{out}");
}

#[test]
fn category_context_persists_with_sqlite_backend() {
    let trt = Trt::new();
    trt.ok(&["config", "set", "storage.type=sqlite"]);

    trt.ok(&["category", "add", "Work"]);
    trt.ok(&["category", "use", "Work"]);

    let out = trt.ok(&["category", "show"]);
    assert!(out.contains("Current category: Work (ID: 1)"), "{out}");
}

/// `category list`'s output order, one line per category, for asserting
/// against without depending on the leading "Categories:" header.
fn list_order(out: &str) -> Vec<String> {
    out.lines()
        .skip(1) // "Categories:"
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

#[test]
fn category_order_changes_list_position() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]); // ID 1, default order 1
    trt.ok(&["category", "add", "Home"]); // ID 2, default order 2

    // Before: Uncategorized, Work, Home
    let out = list_order(&trt.ok(&["category", "list"]));
    assert_eq!(
        out,
        vec!["0: Uncategorized (current)", "1: Work", "2: Home"]
    );

    // Move Home ahead of Work.
    let msg = trt.ok(&["category", "order", "Home", "1"]);
    assert!(msg.contains("Home") && msg.contains("position 1"), "{msg}");

    // Home (order 1) now ties Work's default order (1); "Home" < "Work"
    // alphabetically, so Home sorts first among the tied pair. IDs never
    // move, only list position does - Home stays ID 2, Work stays ID 1.
    let out = list_order(&trt.ok(&["category", "list"]));
    assert_eq!(
        out,
        vec!["0: Uncategorized (current)", "2: Home", "1: Work"]
    );
}

#[test]
fn category_order_works_by_id() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]); // ID 1
    trt.ok(&["category", "add", "Home"]); // ID 2

    trt.ok(&["category", "order", "2", "1"]);
    let out = list_order(&trt.ok(&["category", "list"]));
    assert_eq!(
        out,
        vec!["0: Uncategorized (current)", "2: Home", "1: Work"]
    );
}

#[test]
fn category_reorder_sets_several_at_once() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]); // ID 1
    trt.ok(&["category", "add", "Home"]); // ID 2
    trt.ok(&["category", "add", "Errands"]); // ID 3

    let msg = trt.ok(&["category", "reorder", "Errands", "Home", "Work"]);
    assert!(
        msg.contains("Errands, Home, Work"),
        "expected the reordered names echoed back: {msg}"
    );

    let out = list_order(&trt.ok(&["category", "list"]));
    assert_eq!(
        out,
        vec![
            "0: Uncategorized (current)",
            "3: Errands",
            "2: Home",
            "1: Work"
        ]
    );
}

#[test]
fn category_reorder_partial_list_leaves_others_where_they_were() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]); // ID 1, default order 1
    trt.ok(&["category", "add", "Home"]); // ID 2, default order 2
    trt.ok(&["category", "add", "Errands"]); // ID 3, default order 3

    // Only reorder Home and Errands; Work is left with its default order
    // (1), which ties Errands' newly assigned order (also 1).
    trt.ok(&["category", "reorder", "Errands", "Home"]);

    let out = list_order(&trt.ok(&["category", "list"]));
    // Errands and Work both land on order 1; "Errands" < "Work"
    // alphabetically breaks the tie. Home was assigned order 2.
    assert_eq!(
        out,
        vec![
            "0: Uncategorized (current)",
            "3: Errands",
            "1: Work",
            "2: Home"
        ]
    );
}

#[test]
fn category_order_rejects_uncategorized() {
    let trt = Trt::new();

    let err = trt.fail(&["category", "order", "Uncategorized", "1"]);
    assert!(err.contains("cannot be reordered"), "{err}");

    let err = trt.fail(&["category", "order", "0", "1"]);
    assert!(err.contains("cannot be reordered"), "{err}");

    let err = trt.fail(&["category", "reorder", "Uncategorized"]);
    assert!(err.contains("cannot be reordered"), "{err}");
}

#[test]
fn category_order_rejects_invalid_positions() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);

    // 0 is out of range: positions are 1-based.
    let err = trt.fail(&["category", "order", "Work", "0"]);
    assert!(err.contains("1-based"), "{err}");

    // Not a number at all - rejected by clap before it reaches `main`.
    trt.fail(&["category", "order", "Work", "nope"]);

    // Unknown category name is a clean error, not a panic.
    let err = trt.fail(&["category", "order", "NoSuchCategory", "1"]);
    assert!(err.contains("NoSuchCategory"), "{err}");
}

#[test]
fn category_order_persists_with_sqlite_backend() {
    let trt = Trt::new();
    trt.ok(&["config", "set", "storage.type=sqlite"]);

    trt.ok(&["category", "add", "Work"]); // ID 1
    trt.ok(&["category", "add", "Home"]); // ID 2
    trt.ok(&["category", "order", "Home", "1"]);

    let out = list_order(&trt.ok(&["category", "list"]));
    assert_eq!(
        out,
        vec!["0: Uncategorized (current)", "2: Home", "1: Work"]
    );
}

#[test]
fn category_reorder_persists_with_sqlite_backend() {
    let trt = Trt::new();
    trt.ok(&["config", "set", "storage.type=sqlite"]);

    trt.ok(&["category", "add", "Work"]); // ID 1
    trt.ok(&["category", "add", "Home"]); // ID 2
    trt.ok(&["category", "add", "Errands"]); // ID 3

    trt.ok(&["category", "reorder", "Errands", "Home", "Work"]);

    let out = list_order(&trt.ok(&["category", "list"]));
    assert_eq!(
        out,
        vec![
            "0: Uncategorized (current)",
            "3: Errands",
            "2: Home",
            "1: Work"
        ]
    );
}
