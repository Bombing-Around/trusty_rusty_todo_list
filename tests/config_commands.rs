//! End-to-end coverage of `trt config ...`, pinning down that declared
//! config defaults are actually applied.
//!
//! Every invocation is pointed at a `--config` file inside a `TempDir`, and
//! `$HOME` is pointed at a path that doesn't exist, so the real
//! `~/.config/trt` and the developer's actual config/todo data are never
//! read or written.

use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

struct Trt {
    _dir: TempDir,
    config_path: std::path::PathBuf,
}

impl Trt {
    /// A fresh instance with no config file yet - this is the "clean $HOME"
    /// scenario the issue is about, so unlike `category_commands.rs` this
    /// constructor deliberately does *not* seed any config.
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let config_path = dir.path().join("trt-config.json");
        Self {
            config_path,
            _dir: dir,
        }
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
}

/// Parses `config list` output lines of the form `*key = value` /
/// ` key = value` into `(key, value, is_default)`.
fn parse_list(output: &str) -> Vec<(String, String, bool)> {
    output
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let is_default = line.starts_with('*');
            let rest = &line[1..];
            let (key, value) = rest.split_once(" = ").expect("malformed list line");
            (key.to_string(), value.to_string(), is_default)
        })
        .collect()
}

fn find<'a>(entries: &'a [(String, String, bool)], key: &str) -> &'a (String, String, bool) {
    entries
        .iter()
        .find(|(k, _, _)| k == key)
        .unwrap_or_else(|| panic!("key {key} missing from config list output"))
}

/// A fresh install (no config file ever written) must report the README's
/// documented defaults, each marked with `*` - not `null`. This is the
/// primary regression test for defaults being declared but never applied.
#[test]
fn fresh_install_reports_documented_defaults() {
    let trt = Trt::new();
    assert!(
        !trt.config_path.exists(),
        "test setup should not have created a config file"
    );

    let out = trt.ok(&["config", "list"]);
    let entries = parse_list(&out);
    assert_eq!(entries.len(), 5, "{out}");

    assert_eq!(
        find(&entries, "deleted-task-lifespan"),
        &("deleted-task-lifespan".to_string(), "0".to_string(), true)
    );
    assert_eq!(
        find(&entries, "storage.type"),
        &("storage.type".to_string(), "json".to_string(), true)
    );
    let (_, storage_path, is_default) = find(&entries, "storage.path");
    assert!(*is_default, "{out}");
    assert!(
        storage_path.ends_with("/.config/trt"),
        "expected an expanded ~/.config/trt path, got {storage_path}"
    );
    assert_eq!(
        find(&entries, "default-category"),
        &("default-category".to_string(), "null".to_string(), true)
    );
    assert_eq!(
        find(&entries, "default-priority"),
        &("default-priority".to_string(), "medium".to_string(), true)
    );
}

/// Regression test for the second, easier-to-miss half of that bug: once the
/// config file exists (because *one* key was set), every *other* key must
/// still report as a default on the next invocation - not as an explicit
/// `null`. A `Default` impl alone fixes the fresh-install case above but not
/// this one, since serde would otherwise see the untouched keys as present
/// (holding a literal `null`) rather than absent.
#[test]
fn set_one_key_leaves_others_reporting_their_defaults() {
    let trt = Trt::new();

    trt.ok(&["config", "set", "default-priority=high"]);
    assert!(trt.config_path.exists());

    let out = trt.ok(&["config", "list"]);
    let entries = parse_list(&out);

    // The key we set shows its value, without the `*`.
    assert_eq!(
        find(&entries, "default-priority"),
        &("default-priority".to_string(), "high".to_string(), false)
    );

    // Every other key is untouched and must still report as a default.
    assert_eq!(
        find(&entries, "deleted-task-lifespan"),
        &("deleted-task-lifespan".to_string(), "0".to_string(), true)
    );
    assert_eq!(
        find(&entries, "storage.type"),
        &("storage.type".to_string(), "json".to_string(), true)
    );
    assert!(find(&entries, "storage.path").2, "{out}");
    assert_eq!(
        find(&entries, "default-category"),
        &("default-category".to_string(), "null".to_string(), true)
    );
}

/// `config default <key>` must restore the documented default, not `null`.
#[test]
fn config_default_restores_documented_default() {
    let trt = Trt::new();

    trt.ok(&["config", "set", "default-priority=high"]);
    let out = trt.ok(&["config", "list"]);
    assert_eq!(
        find(&parse_list(&out), "default-priority"),
        &("default-priority".to_string(), "high".to_string(), false)
    );

    trt.ok(&["config", "default", "default-priority"]);
    let out = trt.ok(&["config", "list"]);
    assert_eq!(
        find(&parse_list(&out), "default-priority"),
        &("default-priority".to_string(), "medium".to_string(), true)
    );
}
