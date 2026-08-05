//! End-to-end coverage of the first-run offer to create the default "Home"
//! and "Work" categories.
//!
//! Every invocation is pointed at a `--config` file inside a `TempDir`, with
//! `storage.path` pointing at that same `TempDir` and `$HOME` pointing at a
//! path that does not exist, so the real `~/.config/trt` is never read or
//! written - which matters more here than anywhere else, since this is the
//! one feature whose whole job is to write to a *fresh* install.
//!
//! `cargo test` runs the binary with a piped stdin, so there is never a
//! terminal attached: every test here exercises a path that must not block.
//! The interactive answer itself is supplied by `--yes` / `--no-input`
//! (`StdinPrompter`'s own y/n parsing is unit-tested in `src/prompter.rs`).

use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

struct Trt {
    _dir: TempDir,
    config_path: PathBuf,
    data_dir: PathBuf,
}

impl Trt {
    /// A fresh install: a config file that only says where task data lives,
    /// and no task data of any kind yet.
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("trt-config.json");
        let data_dir = dir.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        let this = Self {
            config_path,
            data_dir,
            _dir: dir,
        };
        // `config set` is itself a command that never opens task storage, so
        // this setup step cannot trigger the offer it is setting up for.
        this.ok(&[
            "config",
            "set",
            &format!("storage.path={}", this.data_dir.display()),
        ]);
        this
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_trt"))
            .arg("--config")
            .arg(&self.config_path)
            // Make sure nothing can silently fall back to a real home directory.
            .env("HOME", Path::new("/nonexistent-trt-test-home"))
            .args(args)
            .output()
            .expect("failed to run trt")
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

    /// The JSON data file the default backend writes to. Its *absence* is
    /// the strongest available assertion that a command left task storage
    /// completely alone.
    fn data_file(&self) -> PathBuf {
        self.data_dir.join("trt-data.json")
    }

    /// Whether the first-run offer has been recorded as resolved. Read
    /// straight from the config file rather than through the CLI: the marker
    /// is deliberately not a user-facing config key, so `config list` cannot
    /// (and should not) show it.
    fn offer_recorded(&self) -> bool {
        let contents = std::fs::read_to_string(&self.config_path).unwrap_or_default();
        let config: serde_json::Value = serde_json::from_str(&contents).unwrap_or_default();
        config
            .get("default_categories_offered")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }

    /// The category names `category list` reports, minus the synthesized
    /// "Uncategorized" entry that is always present.
    fn real_categories(&self) -> Vec<String> {
        self.ok(&["category", "list"])
            .lines()
            .filter_map(|line| line.split_once(": "))
            .map(|(_, rest)| {
                // Strips the trailing "(current)" marker and the
                // "(description: ...)" suffix `category list` always
                // appends, leaving just the name.
                rest.split_once(" (description:")
                    .map(|(name, _)| name)
                    .unwrap_or(rest)
                    .trim_end_matches(" (current)")
                    .to_string()
            })
            .filter(|name| name != "Uncategorized")
            .collect()
    }
}

/// Accepting the offer creates exactly the two documented categories, and
/// only ever once: a second run must not re-offer (which would fail outright
/// on the duplicate-name check) or create anything more.
#[test]
fn accepting_creates_home_and_work_exactly_once() {
    let trt = Trt::new();
    assert!(!trt.offer_recorded());

    let out = trt.ok(&["--yes", "category", "list"]);
    assert!(out.contains("Category 'Home' added with ID 1"), "{out}");
    assert!(out.contains("Category 'Work' added with ID 2"), "{out}");
    assert!(trt.offer_recorded());

    // Every subsequent run - with or without `--yes` - is a no-op.
    let out = trt.ok(&["--yes", "category", "list"]);
    assert!(!out.contains("added with ID"), "{out}");
    assert_eq!(trt.real_categories(), vec!["Home", "Work"]);
}

/// Declining creates neither category, and is a real answer: it is recorded,
/// so no later run asks again - not even one that would have said yes.
#[test]
fn declining_creates_nothing_and_is_never_asked_again() {
    let trt = Trt::new();

    let out = trt.ok(&["--no-input", "category", "list"]);
    assert!(!out.contains("added with ID"), "{out}");
    assert!(out.contains("add categories yourself"), "{out}");
    assert!(trt.offer_recorded());

    assert!(trt.real_categories().is_empty());
    trt.ok(&["--yes", "list"]);
    assert!(trt.real_categories().is_empty());
}

/// With no terminal attached and no flag saying what to assume, there is
/// nobody to ask: the command must run to completion without blocking,
/// without creating categories, and without printing anything about the
/// offer (a script would see that message on every run until it was
/// answered).
///
/// It must also not *record* the offer - nobody answered it - so the same
/// install still gets the offer once a human runs it interactively, which
/// the `--yes` run at the end stands in for.
#[test]
fn a_non_interactive_run_neither_blocks_nor_burns_the_offer() {
    let trt = Trt::new();

    let out = trt.ok(&["list"]);
    assert_eq!(out, "Tasks:\n", "{out}");
    assert!(!trt.offer_recorded());
    assert!(
        !trt.data_file().exists(),
        "a first-run offer nobody could answer must not write task storage"
    );

    let out = trt.ok(&["--yes", "category", "list"]);
    assert!(out.contains("Category 'Home' added with ID 1"), "{out}");
}

/// `trt config ...` never opens task storage, so it must never make the
/// offer either - creating categories from a read-only config query would
/// write a data file (and its directory) as a side effect, and offering
/// during `config set storage.path=...` would put the new categories in the
/// location the user is in the middle of moving away from.
#[test]
fn config_commands_never_trigger_the_offer() {
    let trt = Trt::new();

    trt.ok(&["config", "list"]);
    trt.ok(&["--yes", "config", "list"]);
    trt.ok(&["--yes", "config", "set", "default-priority=high"]);

    assert!(!trt.offer_recorded());
    assert!(!trt.data_file().exists());

    // The offer is not lost, just deferred to the first command that
    // actually touches task storage.
    let out = trt.ok(&["--yes", "category", "list"]);
    assert!(out.contains("Category 'Home' added with ID 1"), "{out}");
}

/// An install that already has categories is not a fresh one, so it is never
/// interrupted by the offer - and the offer is quietly marked resolved, which
/// is what keeps "the user deliberately deleted every category" from looking
/// like a fresh install later on.
#[test]
fn existing_categories_mean_this_is_not_a_first_run() {
    let trt = Trt::new();

    // Created by a non-interactive run, so nothing has been offered or
    // recorded yet - only this one category exists.
    trt.ok(&["category", "add", "Errands"]);
    assert!(!trt.offer_recorded());

    let out = trt.ok(&["--yes", "category", "list"]);
    assert!(!out.contains("added with ID"), "{out}");
    assert_eq!(trt.real_categories(), vec!["Errands"]);
    assert!(trt.offer_recorded());
}

/// Deleting every category is a legitimate empty state, not a reinstall: the
/// recorded marker means it is never mistaken for one.
#[test]
fn deleting_every_category_does_not_re_offer() {
    let trt = Trt::new();
    trt.ok(&["--yes", "category", "list"]);

    trt.ok(&["category", "delete", "Home"]);
    trt.ok(&["category", "delete", "Work"]);
    assert!(trt.real_categories().is_empty());

    let out = trt.ok(&["--yes", "category", "list"]);
    assert!(!out.contains("added with ID"), "{out}");
    assert!(trt.real_categories().is_empty());
}

/// The offer works the same way against the SQLite backend - it goes through
/// `CategoryManager`, not any backend-specific path.
#[test]
fn the_offer_is_backend_agnostic() {
    let trt = Trt::new();
    trt.ok(&["config", "set", "storage.type=sqlite"]);

    let out = trt.ok(&["--yes", "category", "list"]);
    assert!(out.contains("Category 'Home' added with ID 1"), "{out}");
    assert_eq!(trt.real_categories(), vec!["Home", "Work"]);
}
