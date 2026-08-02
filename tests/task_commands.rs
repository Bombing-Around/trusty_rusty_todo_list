//! End-to-end coverage of `trtodo add/delete/update/check/uncheck/move/list`.
//!
//! Every invocation is pointed at a `--config` file inside a `TempDir`, and
//! that config points `storage.path` at the same `TempDir`, so the real
//! `~/.config/trtodo` and the developer's actual todo data are never read or
//! written. This mirrors `tests/category_commands.rs`'s harness exactly.

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
fn add_defaults_priority_from_config_and_lists_it() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);

    // No --priority: falls back to the configured/documented default
    // (medium), not a hardcoded value (issue #22).
    let out = trtodo.ok(&["add", "Buy milk", "--category", "Work"]);
    assert!(out.contains("added with ID 1"), "{out}");

    let out = trtodo.ok(&["list"]);
    assert!(
        out.contains("1: [ ] Buy milk (priority: medium, category: Work)"),
        "{out}"
    );

    // Explicit --priority overrides the default.
    trtodo.ok(&[
        "add",
        "Walk dog",
        "--category",
        "Work",
        "--priority",
        "high",
    ]);
    let out = trtodo.ok(&["list"]);
    assert!(
        out.contains("2: [ ] Walk dog (priority: high, category: Work)"),
        "{out}"
    );

    // Changing the configured default changes what a subsequent bare `add`
    // picks up.
    trtodo.ok(&["config", "set", "default-priority=low"]);
    trtodo.ok(&["add", "Water plants", "--category", "Work"]);
    let out = trtodo.ok(&["list"]);
    assert!(
        out.contains("3: [ ] Water plants (priority: low, category: Work)"),
        "{out}"
    );
}

#[test]
fn add_can_target_the_uncategorized_category_by_name_or_id() {
    let trtodo = Trtodo::new();

    trtodo.ok(&["add", "Loose task", "--category", "Uncategorized"]);
    let out = trtodo.ok(&["list"]);
    assert!(out.contains("category: Uncategorized"), "{out}");

    trtodo.ok(&["add", "Another loose task", "--category", "0"]);
    let out = trtodo.ok(&["list"]);
    assert!(out.contains("Another loose task"), "{out}");
}

#[test]
fn list_filters_by_search_completed_and_priority() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&[
        "add",
        "Buy milk",
        "--category",
        "Work",
        "--priority",
        "high",
    ]);
    trtodo.ok(&[
        "add",
        "Buy bread",
        "--category",
        "Work",
        "--priority",
        "low",
    ]);
    trtodo.ok(&["check", "Buy milk", "--category", "Work"]);

    let out = trtodo.ok(&["list", "--search", "bread"]);
    assert!(out.contains("Buy bread"), "{out}");
    assert!(!out.contains("Buy milk"), "{out}");

    let out = trtodo.ok(&["list", "--completed"]);
    assert!(out.contains("Buy milk"), "{out}");
    assert!(!out.contains("Buy bread"), "{out}");

    let out = trtodo.ok(&["list", "--priority", "low"]);
    assert!(out.contains("Buy bread"), "{out}");
    assert!(!out.contains("Buy milk"), "{out}");
}

#[test]
fn check_and_uncheck_by_explicit_category_and_by_context() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);

    // Explicit --category.
    trtodo.ok(&["check", "Buy milk", "--category", "Work"]);
    let out = trtodo.ok(&["list"]);
    assert!(out.contains("[x] Buy milk"), "{out}");

    trtodo.ok(&["uncheck", "Buy milk", "--category", "Work"]);
    let out = trtodo.ok(&["list"]);
    assert!(out.contains("[ ] Buy milk"), "{out}");

    // Falls back to the current category context when --category is
    // omitted (README).
    trtodo.ok(&["category", "use", "Work"]);
    trtodo.ok(&["check", "Buy milk"]);
    let out = trtodo.ok(&["list"]);
    assert!(out.contains("[x] Buy milk"), "{out}");

    // Without --category and without a context, resolution now searches
    // unscoped across every category (README's cross-category matching)
    // instead of erroring - and since "Buy milk" is unambiguous here (it
    // only exists in Work), that succeeds cleanly rather than requiring a
    // context.
    trtodo.ok(&["category", "clear"]);
    let out = trtodo.ok(&["check", "Buy milk"]);
    assert!(out.contains("checked off"), "{out}");
}

#[test]
fn check_all_and_uncheck_all_require_and_use_the_category_context() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["category", "add", "Home"]);
    trtodo.ok(&["add", "Work task", "--category", "Work"]);
    trtodo.ok(&["add", "Home task", "--category", "Home"]);

    // No context set: check-all/uncheck-all have no --category argument at
    // all, so this must fail cleanly rather than guess.
    let err = trtodo.fail(&["check-all"]);
    assert!(err.contains("no category context"), "{err}");
    // The advice must be actionable: `check-all` accepts no --category, so
    // the message must not tell the user to pass one (it used to).
    assert!(err.contains("category use"), "{err}");
    assert!(!err.contains("--category"), "{err}");

    trtodo.ok(&["category", "use", "Work"]);
    let out = trtodo.ok(&["check-all"]);
    assert!(out.contains("Checked off 1 task(s)"), "{out}");

    let out = trtodo.ok(&["list"]);
    assert!(out.contains("[x] Work task"), "{out}");
    assert!(out.contains("[ ] Home task"), "{out}");

    let out = trtodo.ok(&["uncheck-all"]);
    assert!(out.contains("Unchecked 1 task(s)"), "{out}");
    let out = trtodo.ok(&["list"]);
    assert!(out.contains("[ ] Work task"), "{out}");
}

#[test]
fn move_simple_syntax_uses_category_context() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["category", "add", "Home"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);

    // No context: the simple syntax has no --category at all, so it needs
    // an explicit context first.
    let err = trtodo.fail(&["move", "Buy milk", "--to", "Home"]);
    assert!(err.contains("no category context"), "{err}");
    // `move` has no --category either, but it does have an escape hatch the
    // message should point at: the extended --from/--task form.
    assert!(!err.contains("--category"), "{err}");
    assert!(err.contains("--from"), "{err}");

    trtodo.ok(&["category", "use", "Work"]);
    let out = trtodo.ok(&["move", "Buy milk", "--to", "Home"]);
    assert!(out.contains("moved to 'Home'"), "{out}");

    let out = trtodo.ok(&["list"]);
    assert!(out.contains("category: Home"), "{out}");
}

#[test]
fn move_extended_syntax_and_omitted_to_goes_to_uncategorized() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["category", "add", "Home"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);

    // Extended syntax with an explicit --to.
    let out = trtodo.ok(&[
        "move", "--from", "Work", "--to", "Home", "--task", "Buy milk",
    ]);
    assert!(out.contains("moved to 'Home'"), "{out}");
    let out = trtodo.ok(&["list"]);
    assert!(out.contains("category: Home"), "{out}");

    // Omitting --to moves the task to Uncategorized (README).
    let out = trtodo.ok(&["move", "--from", "Home", "--task", "Buy milk"]);
    assert!(out.contains("moved to 'Uncategorized'"), "{out}");
    let out = trtodo.ok(&["list"]);
    assert!(out.contains("category: Uncategorized"), "{out}");
}

#[test]
fn move_rejects_incoherent_argument_combinations() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);

    // Neither syntax: no task reference at all.
    let err = trtodo.fail(&["move", "--to", "Work"]);
    assert!(err.contains("invalid 'move' arguments"), "{err}");

    // --task without the required --from.
    let err = trtodo.fail(&["move", "--task", "Buy milk", "--to", "Work"]);
    assert!(err.contains("invalid 'move' arguments"), "{err}");

    // Simple syntax missing its required --to.
    let err = trtodo.fail(&["move", "Buy milk"]);
    assert!(err.contains("invalid 'move' arguments"), "{err}");
}

#[test]
fn delete_is_soft_and_the_task_disappears_from_list() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);
    trtodo.ok(&["add", "Walk dog", "--category", "Work"]);

    let out = trtodo.ok(&["delete", "Buy milk", "--category", "Work"]);
    assert!(out.contains("deleted"), "{out}");

    let out = trtodo.ok(&["list"]);
    assert!(!out.contains("Buy milk"), "{out}");
    assert!(out.contains("Walk dog"), "{out}");

    // Deleting an already-nonexistent task is a clean error, not a panic.
    let err = trtodo.fail(&["delete", "Buy milk", "--category", "Work"]);
    assert!(err.starts_with("error: "), "{err}");
}

#[test]
fn update_renames_a_task() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);

    let out = trtodo.ok(&[
        "update",
        "Buy milk",
        "--to",
        "Buy oat milk",
        "--category",
        "Work",
    ]);
    assert!(out.contains("renamed to 'Buy oat milk'"), "{out}");

    let out = trtodo.ok(&["list"]);
    assert!(out.contains("Buy oat milk"), "{out}");
    assert!(!out.contains("Buy milk\n"), "{out}");
}

#[test]
fn same_task_name_in_different_categories_is_disambiguated_by_category() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["category", "add", "Home"]);
    trtodo.ok(&["add", "Call mom", "--category", "Work"]);
    trtodo.ok(&["add", "Call mom", "--category", "Home"]);

    // Scoping to one category resolves cleanly - no prompt needed since
    // --category already disambiguates cross-category duplicates.
    trtodo.ok(&["check", "Call mom", "--category", "Work"]);
    let out = trtodo.ok(&["list"]);
    // Exactly one "Call mom" line is checked, the other is not.
    let checked_lines: Vec<&str> = out
        .lines()
        .filter(|l| l.contains("Call mom") && l.contains("[x]"))
        .collect();
    assert_eq!(checked_lines.len(), 1, "{out}");
}

#[test]
fn same_task_name_in_one_category_without_a_terminal_is_a_clean_error() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["add", "Duplicate", "--category", "Work"]);
    trtodo.ok(&["add", "Duplicate", "--category", "Work"]);

    // Two tasks share both name *and* category: `--category` cannot
    // disambiguate between them, and the test harness has no real
    // terminal attached, so this must fail cleanly (README's prompt,
    // detected as non-interactive) instead of hanging or panicking.
    let err = trtodo.fail(&["check", "Duplicate", "--category", "Work"]);
    assert!(err.contains("no terminal is attached"), "{err}");

    // The task ID still works to disambiguate directly.
    let out = trtodo.ok(&["check", "2", "--category", "Work"]);
    assert!(out.contains("checked off"), "{out}");
}

#[test]
fn same_task_name_across_categories_with_no_context_reaches_the_disambiguation_prompt() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["category", "add", "Work"]);
    trtodo.ok(&["category", "add", "Home"]);
    trtodo.ok(&["add", "Call mom", "--category", "Work"]);
    trtodo.ok(&["add", "Call mom", "--category", "Home"]);

    // No --category and no context set: resolution is unscoped across every
    // category, so the two same-named tasks in different categories collide
    // - this is the README's cross-category disambiguation path, previously
    // unreachable because these commands always required a category scope.
    // The test harness has no real terminal attached, so the prompt
    // surfaces as a clean `PromptError::NotInteractive`, not `NoCategoryContext`.
    let err = trtodo.fail(&["check", "Call mom"]);
    assert!(err.contains("no terminal is attached"), "{err}");

    let err = trtodo.fail(&["delete", "Call mom"]);
    assert!(err.contains("no terminal is attached"), "{err}");

    let err = trtodo.fail(&["update", "Call mom", "--to", "Call dad"]);
    assert!(err.contains("no terminal is attached"), "{err}");

    // The task ID still disambiguates directly, with no category needed.
    let out = trtodo.ok(&["check", "1"]);
    assert!(out.contains("checked off"), "{out}");
}

#[test]
fn task_lifecycle_end_to_end_with_sqlite_backend() {
    let trtodo = Trtodo::new();
    trtodo.ok(&["config", "set", "storage.type=sqlite"]);
    trtodo.ok(&["category", "add", "Work"]);

    trtodo.ok(&["add", "Buy milk", "--category", "Work"]);
    let out = trtodo.ok(&["list"]);
    assert!(out.contains("Buy milk"), "{out}");

    trtodo.ok(&["check", "Buy milk", "--category", "Work"]);
    let out = trtodo.ok(&["list", "--completed"]);
    assert!(out.contains("Buy milk"), "{out}");

    trtodo.ok(&["delete", "Buy milk", "--category", "Work"]);
    let out = trtodo.ok(&["list"]);
    assert!(!out.contains("Buy milk"), "{out}");
}
