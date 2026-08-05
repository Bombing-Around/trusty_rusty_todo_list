//! End-to-end coverage of `trt add/delete/update/check/uncheck/move/list`.
//!
//! Every invocation is pointed at a `--config` file inside a `TempDir`, and
//! that config points `storage.path` at the same `TempDir`, so the real
//! `~/.config/trt` and the developer's actual todo data are never read or
//! written. This mirrors `tests/category_commands.rs`'s harness exactly.

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
    /// instead of touching the developer's real config/todo data.
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
fn add_defaults_priority_from_config_and_lists_it() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);

    // No --priority: falls back to the configured/documented default
    // (medium), not a hardcoded value.
    let out = trt.ok(&["add", "Buy milk", "--category", "Work"]);
    assert!(out.contains("added with ID 1"), "{out}");

    let out = trt.ok(&["list"]);
    assert!(
        out.contains("1: [ ] Buy milk (priority: medium, category: Work)"),
        "{out}"
    );

    // Explicit --priority overrides the default.
    trt.ok(&[
        "add",
        "Walk dog",
        "--category",
        "Work",
        "--priority",
        "high",
    ]);
    let out = trt.ok(&["list"]);
    assert!(
        out.contains("2: [ ] Walk dog (priority: high, category: Work)"),
        "{out}"
    );

    // Changing the configured default changes what a subsequent bare `add`
    // picks up.
    trt.ok(&["config", "set", "default-priority=low"]);
    trt.ok(&["add", "Water plants", "--category", "Work"]);
    let out = trt.ok(&["list"]);
    assert!(
        out.contains("3: [ ] Water plants (priority: low, category: Work)"),
        "{out}"
    );
}

/// `--category` is optional on `add`, and the four resolution steps must be
/// tried in order, most specific first.
#[test]
fn add_resolves_its_category_in_precedence_order() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&["category", "add", "Home"]);
    trt.ok(&["category", "add", "Errands"]);

    // Step 4: nothing set at all - no --category, no context, no
    // default-category - so the task is simply uncategorized rather than an
    // error.
    let out = trt.ok(&["add", "Loose task"]);
    assert!(out.contains("in category 'Uncategorized'"), "{out}");

    // Step 3: `default-category` now supplies it. This is the step that did
    // not exist before - the setting was stored, validated and listed, but
    // never read by anything.
    trt.ok(&["config", "set", "default-category=Errands"]);
    let out = trt.ok(&["add", "Configured task"]);
    assert!(out.contains("in category 'Errands'"), "{out}");

    // Step 2: an explicit `category use` context outranks the configured
    // default - transient intent beats persistent configuration.
    trt.ok(&["category", "use", "Home"]);
    let out = trt.ok(&["add", "Context task"]);
    assert!(out.contains("in category 'Home'"), "{out}");

    // Step 1: an explicit --category outranks both.
    let out = trt.ok(&["add", "Explicit task", "--category", "Work"]);
    assert!(out.contains("in category 'Work'"), "{out}");

    // Clearing the context falls back to the configured default again, rather
    // than to Uncategorized - the two settings do not consume each other.
    trt.ok(&["category", "clear"]);
    let out = trt.ok(&["add", "Back to default"]);
    assert!(out.contains("in category 'Errands'"), "{out}");

    // And unsetting the default falls the rest of the way to Uncategorized.
    trt.ok(&["config", "default", "default-category"]);
    let out = trt.ok(&["add", "Truly loose"]);
    assert!(out.contains("in category 'Uncategorized'"), "{out}");

    let out = trt.ok(&["list"]);
    assert!(
        out.contains("Loose task (priority: medium, category: Uncategorized)"),
        "{out}"
    );
    assert!(
        out.contains("Configured task (priority: medium, category: Errands)"),
        "{out}"
    );
    assert!(
        out.contains("Context task (priority: medium, category: Home)"),
        "{out}"
    );
    assert!(
        out.contains("Explicit task (priority: medium, category: Work)"),
        "{out}"
    );
}

/// `default-category` is stored without being validated against the category
/// list, and a category that existed when it was set can be deleted later, so
/// `add` has to cope with a default that does not resolve. It refuses rather
/// than quietly filing the task under Uncategorized.
#[test]
fn add_errors_when_the_configured_default_category_does_not_resolve() {
    let trt = Trt::new();

    // A name that never existed - `config set` accepts it happily.
    trt.ok(&["config", "set", "default-category=Ghost"]);
    let err = trt.fail(&["add", "Orphaned task"]);
    assert!(err.contains("default-category"), "{err}");
    assert!(err.contains("Ghost"), "{err}");
    // The message has to be actionable, since the user cannot see which of the
    // four steps was reached.
    assert!(err.contains("--category"), "{err}");

    // Nothing was written: refusing must not half-succeed.
    let out = trt.ok(&["list"]);
    assert!(!out.contains("Orphaned task"), "{out}");

    // A broken default only breaks the `add`s that actually fall through to
    // it - an explicit --category still works.
    trt.ok(&["category", "add", "Work"]);
    let out = trt.ok(&["add", "Explicit task", "--category", "Work"]);
    assert!(out.contains("in category 'Work'"), "{out}");

    // ...as does a `category use` context, which is resolved one step earlier.
    trt.ok(&["category", "use", "Work"]);
    let out = trt.ok(&["add", "Context task"]);
    assert!(out.contains("in category 'Work'"), "{out}");

    // A default that was valid when set but whose category has since been
    // deleted behaves the same way - which is why validating at `config set`
    // time would not have been enough.
    trt.ok(&["category", "clear"]);
    trt.ok(&["category", "add", "Temporary"]);
    trt.ok(&["config", "set", "default-category=Temporary"]);
    trt.ok(&["add", "Fine for now"]);
    trt.ok(&["category", "delete", "Temporary"]);
    let err = trt.fail(&["add", "Too late"]);
    assert!(err.contains("Temporary"), "{err}");
}

/// `default-category` may name a category by ID as well as by name, since it
/// goes through the same resolution as `--category`.
#[test]
fn add_accepts_a_default_category_given_as_an_id() {
    let trt = Trt::new();
    let out = trt.ok(&["category", "add", "Work"]);
    assert!(out.contains("added with ID 1"), "{out}");

    trt.ok(&["config", "set", "default-category=1"]);
    let out = trt.ok(&["add", "By ID"]);
    assert!(out.contains("in category 'Work'"), "{out}");
}

#[test]
fn add_can_target_the_uncategorized_category_by_name_or_id() {
    let trt = Trt::new();

    trt.ok(&["add", "Loose task", "--category", "Uncategorized"]);
    let out = trt.ok(&["list"]);
    assert!(out.contains("category: Uncategorized"), "{out}");

    trt.ok(&["add", "Another loose task", "--category", "0"]);
    let out = trt.ok(&["list"]);
    assert!(out.contains("Another loose task"), "{out}");
}

#[test]
fn list_filters_by_search_completed_and_priority() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&[
        "add",
        "Buy milk",
        "--category",
        "Work",
        "--priority",
        "high",
    ]);
    trt.ok(&[
        "add",
        "Buy bread",
        "--category",
        "Work",
        "--priority",
        "low",
    ]);
    trt.ok(&["check", "Buy milk", "--category", "Work"]);

    let out = trt.ok(&["list", "--search", "bread"]);
    assert!(out.contains("Buy bread"), "{out}");
    assert!(!out.contains("Buy milk"), "{out}");

    let out = trt.ok(&["list", "--completed"]);
    assert!(out.contains("Buy milk"), "{out}");
    assert!(!out.contains("Buy bread"), "{out}");

    let out = trt.ok(&["list", "--priority", "low"]);
    assert!(out.contains("Buy bread"), "{out}");
    assert!(!out.contains("Buy milk"), "{out}");
}

#[test]
fn check_and_uncheck_by_explicit_category_and_by_context() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&["add", "Buy milk", "--category", "Work"]);

    // Explicit --category.
    trt.ok(&["check", "Buy milk", "--category", "Work"]);
    let out = trt.ok(&["list"]);
    assert!(out.contains("[x] Buy milk"), "{out}");

    trt.ok(&["uncheck", "Buy milk", "--category", "Work"]);
    let out = trt.ok(&["list"]);
    assert!(out.contains("[ ] Buy milk"), "{out}");

    // Falls back to the current category context when --category is
    // omitted (README).
    trt.ok(&["category", "use", "Work"]);
    trt.ok(&["check", "Buy milk"]);
    let out = trt.ok(&["list"]);
    assert!(out.contains("[x] Buy milk"), "{out}");

    // Without --category and without a context, resolution now searches
    // unscoped across every category (README's cross-category matching)
    // instead of erroring - and since "Buy milk" is unambiguous here (it
    // only exists in Work), that succeeds cleanly rather than requiring a
    // context.
    trt.ok(&["category", "clear"]);
    let out = trt.ok(&["check", "Buy milk"]);
    assert!(out.contains("checked off"), "{out}");
}

#[test]
fn check_all_and_uncheck_all_require_and_use_the_category_context() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&["category", "add", "Home"]);
    trt.ok(&["add", "Work task", "--category", "Work"]);
    trt.ok(&["add", "Home task", "--category", "Home"]);

    // No context set: check-all/uncheck-all have no --category argument at
    // all, so this must fail cleanly rather than guess.
    let err = trt.fail(&["check-all"]);
    assert!(err.contains("no category context"), "{err}");
    // The advice must be actionable: `check-all` accepts no --category, so
    // the message must not tell the user to pass one (it used to).
    assert!(err.contains("category use"), "{err}");
    assert!(!err.contains("--category"), "{err}");

    trt.ok(&["category", "use", "Work"]);
    let out = trt.ok(&["check-all"]);
    assert!(out.contains("Checked off 1 task(s)"), "{out}");

    let out = trt.ok(&["list"]);
    assert!(out.contains("[x] Work task"), "{out}");
    assert!(out.contains("[ ] Home task"), "{out}");

    let out = trt.ok(&["uncheck-all"]);
    assert!(out.contains("Unchecked 1 task(s)"), "{out}");
    let out = trt.ok(&["list"]);
    assert!(out.contains("[ ] Work task"), "{out}");
}

#[test]
fn move_simple_syntax_uses_category_context() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&["category", "add", "Home"]);
    trt.ok(&["add", "Buy milk", "--category", "Work"]);

    // No context: the simple syntax has no --category at all, so it needs
    // an explicit context first.
    let err = trt.fail(&["move", "Buy milk", "--to", "Home"]);
    assert!(err.contains("no category context"), "{err}");
    // `move` has no --category either, but it does have an escape hatch the
    // message should point at: the extended --from/--task form.
    assert!(!err.contains("--category"), "{err}");
    assert!(err.contains("--from"), "{err}");

    trt.ok(&["category", "use", "Work"]);
    let out = trt.ok(&["move", "Buy milk", "--to", "Home"]);
    assert!(out.contains("moved to 'Home'"), "{out}");

    let out = trt.ok(&["list"]);
    assert!(out.contains("category: Home"), "{out}");
}

#[test]
fn move_extended_syntax_and_omitted_to_goes_to_uncategorized() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&["category", "add", "Home"]);
    trt.ok(&["add", "Buy milk", "--category", "Work"]);

    // Extended syntax with an explicit --to.
    let out = trt.ok(&[
        "move", "--from", "Work", "--to", "Home", "--task", "Buy milk",
    ]);
    assert!(out.contains("moved to 'Home'"), "{out}");
    let out = trt.ok(&["list"]);
    assert!(out.contains("category: Home"), "{out}");

    // Omitting --to moves the task to Uncategorized (README).
    let out = trt.ok(&["move", "--from", "Home", "--task", "Buy milk"]);
    assert!(out.contains("moved to 'Uncategorized'"), "{out}");
    let out = trt.ok(&["list"]);
    assert!(out.contains("category: Uncategorized"), "{out}");
}

#[test]
fn move_rejects_incoherent_argument_combinations() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&["add", "Buy milk", "--category", "Work"]);

    // Neither syntax: no task reference at all.
    let err = trt.fail(&["move", "--to", "Work"]);
    assert!(err.contains("invalid 'move' arguments"), "{err}");

    // --task without the required --from.
    let err = trt.fail(&["move", "--task", "Buy milk", "--to", "Work"]);
    assert!(err.contains("invalid 'move' arguments"), "{err}");

    // Simple syntax missing its required --to.
    let err = trt.fail(&["move", "Buy milk"]);
    assert!(err.contains("invalid 'move' arguments"), "{err}");
}

#[test]
fn delete_is_soft_and_the_task_disappears_from_list() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&["add", "Buy milk", "--category", "Work"]);
    trt.ok(&["add", "Walk dog", "--category", "Work"]);

    let out = trt.ok(&["delete", "Buy milk", "--category", "Work"]);
    assert!(out.contains("deleted"), "{out}");

    let out = trt.ok(&["list"]);
    assert!(!out.contains("Buy milk"), "{out}");
    assert!(out.contains("Walk dog"), "{out}");

    // Deleting an already-nonexistent task is a clean error, not a panic.
    let err = trt.fail(&["delete", "Buy milk", "--category", "Work"]);
    assert!(err.starts_with("error: "), "{err}");
}

#[test]
fn update_renames_a_task() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&["add", "Buy milk", "--category", "Work"]);

    let out = trt.ok(&[
        "update",
        "Buy milk",
        "--to",
        "Buy oat milk",
        "--category",
        "Work",
    ]);
    assert!(out.contains("renamed to 'Buy oat milk'"), "{out}");

    let out = trt.ok(&["list"]);
    assert!(out.contains("Buy oat milk"), "{out}");
    assert!(!out.contains("Buy milk\n"), "{out}");
}

#[test]
fn same_task_name_in_different_categories_is_disambiguated_by_category() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&["category", "add", "Home"]);
    trt.ok(&["add", "Call mom", "--category", "Work"]);
    trt.ok(&["add", "Call mom", "--category", "Home"]);

    // Scoping to one category resolves cleanly - no prompt needed since
    // --category already disambiguates cross-category duplicates.
    trt.ok(&["check", "Call mom", "--category", "Work"]);
    let out = trt.ok(&["list"]);
    // Exactly one "Call mom" line is checked, the other is not.
    let checked_lines: Vec<&str> = out
        .lines()
        .filter(|l| l.contains("Call mom") && l.contains("[x]"))
        .collect();
    assert_eq!(checked_lines.len(), 1, "{out}");
}

#[test]
fn same_task_name_in_one_category_without_a_terminal_is_a_clean_error() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&["add", "Duplicate", "--category", "Work"]);
    trt.ok(&["add", "Duplicate", "--category", "Work"]);

    // Two tasks share both name *and* category: `--category` cannot
    // disambiguate between them, and the test harness has no real
    // terminal attached, so this must fail cleanly (README's prompt,
    // detected as non-interactive) instead of hanging or panicking.
    let err = trt.fail(&["check", "Duplicate", "--category", "Work"]);
    assert!(err.contains("no terminal is attached"), "{err}");

    // The task ID still works to disambiguate directly.
    let out = trt.ok(&["check", "2", "--category", "Work"]);
    assert!(out.contains("checked off"), "{out}");
}

#[test]
fn same_task_name_across_categories_with_no_context_reaches_the_disambiguation_prompt() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&["category", "add", "Home"]);
    trt.ok(&["add", "Call mom", "--category", "Work"]);
    trt.ok(&["add", "Call mom", "--category", "Home"]);

    // No --category and no context set: resolution is unscoped across every
    // category, so the two same-named tasks in different categories collide
    // - this is the README's cross-category disambiguation path, previously
    // unreachable because these commands always required a category scope.
    // The test harness has no real terminal attached, so the prompt
    // surfaces as a clean `PromptError::NotInteractive`, not `NoCategoryContext`.
    let err = trt.fail(&["check", "Call mom"]);
    assert!(err.contains("no terminal is attached"), "{err}");

    let err = trt.fail(&["delete", "Call mom"]);
    assert!(err.contains("no terminal is attached"), "{err}");

    let err = trt.fail(&["update", "Call mom", "--to", "Call dad"]);
    assert!(err.contains("no terminal is attached"), "{err}");

    // The task ID still disambiguates directly, with no category needed.
    let out = trt.ok(&["check", "1"]);
    assert!(out.contains("checked off"), "{out}");
}

#[test]
fn add_sets_a_description_and_show_displays_it() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&[
        "add",
        "Buy milk",
        "--category",
        "Work",
        "--description",
        "2%, not whole",
    ]);

    let out = trt.ok(&["show", "Buy milk", "--category", "Work"]);
    assert!(out.contains("Description: 2%, not whole"), "{out}");

    // Omitting --description leaves it unset, shown explicitly as "(none)"
    // rather than an absent line - `show` is a detail view, so its whole
    // job is to answer "what is this task's description?" definitively.
    trt.ok(&["add", "Walk dog", "--category", "Work"]);
    let out = trt.ok(&["show", "Walk dog", "--category", "Work"]);
    assert!(out.contains("Description: (none)"), "{out}");
}

#[test]
fn update_can_set_and_clear_a_tasks_description_independently_of_the_title() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&["add", "Buy milk", "--category", "Work"]);

    trt.ok(&[
        "update",
        "Buy milk",
        "--category",
        "Work",
        "--description",
        "Whole milk",
    ]);
    let out = trt.ok(&["show", "Buy milk", "--category", "Work"]);
    assert!(out.contains("Description: Whole milk"), "{out}");

    // Renaming alone (no --description) leaves the description untouched -
    // the whole point of the "double option" plumbing behind this command.
    trt.ok(&[
        "update",
        "Buy milk",
        "--category",
        "Work",
        "--to",
        "Buy oat milk",
    ]);
    let out = trt.ok(&["show", "Buy oat milk", "--category", "Work"]);
    assert!(out.contains("Description: Whole milk"), "{out}");

    // Clearing back to empty requires the explicit flag.
    trt.ok(&[
        "update",
        "Buy oat milk",
        "--category",
        "Work",
        "--clear-description",
    ]);
    let out = trt.ok(&["show", "Buy oat milk", "--category", "Work"]);
    assert!(out.contains("Description: (none)"), "{out}");
}

#[test]
fn update_with_no_fields_at_all_is_a_clean_error() {
    let trt = Trt::new();
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&["add", "Buy milk", "--category", "Work"]);

    // Neither --to, --description, nor --clear-description: there is
    // nothing to update, and that is refused rather than silently
    // succeeding with a misleading "updated" message.
    let err = trt.fail(&["update", "Buy milk", "--category", "Work"]);
    assert!(err.contains("nothing to update"), "{err}");
}

/// Backend parity for the description field: everything above runs against
/// the default JSON backend, this repeats the round trip against SQLite -
/// `save`/`load` are what actually persist the column, and only one backend
/// being exercised is how the SQLite foreign-key bug shipped previously.
#[test]
fn task_description_round_trips_through_the_sqlite_backend() {
    let trt = Trt::new();
    trt.ok(&["config", "set", "storage.type=sqlite"]);
    trt.ok(&["category", "add", "Work"]);
    trt.ok(&[
        "add",
        "Buy milk",
        "--category",
        "Work",
        "--description",
        "2%, not whole",
    ]);

    let out = trt.ok(&["show", "Buy milk", "--category", "Work"]);
    assert!(out.contains("Description: 2%, not whole"), "{out}");

    trt.ok(&[
        "update",
        "Buy milk",
        "--category",
        "Work",
        "--clear-description",
    ]);
    let out = trt.ok(&["show", "Buy milk", "--category", "Work"]);
    assert!(out.contains("Description: (none)"), "{out}");
}

#[test]
fn task_lifecycle_end_to_end_with_sqlite_backend() {
    let trt = Trt::new();
    trt.ok(&["config", "set", "storage.type=sqlite"]);
    trt.ok(&["category", "add", "Work"]);

    trt.ok(&["add", "Buy milk", "--category", "Work"]);
    let out = trt.ok(&["list"]);
    assert!(out.contains("Buy milk"), "{out}");

    trt.ok(&["check", "Buy milk", "--category", "Work"]);
    let out = trt.ok(&["list", "--completed"]);
    assert!(out.contains("Buy milk"), "{out}");

    trt.ok(&["delete", "Buy milk", "--category", "Work"]);
    let out = trt.ok(&["list"]);
    assert!(!out.contains("Buy milk"), "{out}");
}
