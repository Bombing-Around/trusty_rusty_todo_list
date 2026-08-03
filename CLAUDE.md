# trusty_rusty_todo_list

A CLI todo list in Rust. Binary name is `trtodo`. Tasks belong to categories;
configuration and data live under `~/.config/trtodo` by default.

`README.md` is the specification, not just documentation. Where behavior and
README disagree, that is a bug in one of them — decide which, and fix that one.
Several past issues came from code drifting away from what the README promised.

## Commands

```sh
cargo build
cargo test                        # unit + integration
cargo fmt --all -- --check        # exactly what CI runs
cargo clippy -- -D warnings       # exactly what CI runs
```

CI (`.github/workflows/rust.yml`) runs those three plus commitlint. Run all
three before pushing; `clippy -- -D warnings` is stricter than a bare
`cargo clippy` and will fail the build on a warning.

## Layout

| Path | Responsibility |
| --- | --- |
| `src/main.rs` | Thin dispatch. Parses, opens storage, matches commands, prints. Resolution helpers (`resolve_add_category`, `resolve_task_scope`, `require_category_context`) live here because they need both config and storage. |
| `src/cli.rs` | clap definitions only. No behavior. Unit tests assert parsing. |
| `src/config.rs` | Config schema, validation, persistence. Knows nothing about tasks. |
| `src/models/` | `Task`, `Category`, `Priority`, `StorageData`, error types. |
| `src/storage/` | `Storage` trait plus `json` and `sqlite` backends, `config.rs` (config file store), `migrate_storage`. |
| `src/category_manager.rs` | Category CRUD and the `category use` context. |
| `src/task_manager.rs` | Task operations, task resolution, disambiguation. |
| `src/prompter.rs` | The interactive-input seam. See below. |
| `tests/` | Integration tests that shell out to the built binary. |

Rough layering: `main` → managers → `storage` → `models`. `storage` referencing
`category_manager::UNCATEGORIZED_ID` is the one deliberate exception.

## Invariants that bite

These are the things that have actually caused bugs. Read before touching the
related area.

**`Uncategorized` (ID 0) is synthesized, never stored.** `CategoryManager`
builds it on demand; `get_next_category_id` never hands out 0. Consequences:

- `CategoryManager::list_categories` drops any stored row with ID 0 and
  substitutes its own.
- SQLite has a `tasks.category_id` foreign key, so a task in Uncategorized
  references a row that does not otherwise exist. `SqliteStorage::save` seeds a
  sentinel row and `load` filters it back out. **Do not "clean up" either half** —
  removing the seed makes every uncategorized task unstorable under SQLite;
  removing the filter makes a fresh SQLite store look established and silently
  suppresses the first-run offer.

**`has_explicit_category_context()` is not `get_current_category().is_some()`.**
`get_current_category` returns `Some(UNCATEGORIZED_ID)` even when no context was
ever set, so `category show` has something to print. Any code deciding *"did the
user choose a category?"* must use `has_explicit_category_context`. Using the
other one collapses fallback chains silently — this is exactly how
`default-category` came to be ignored for so long.

**Never call `stdin().read_line()` directly.** All interactive input goes through
the `Prompter` trait (`choose`, `confirm`). `StdinPrompter` detects a
non-interactive stdin and returns `PromptError::NotInteractive` instead of
blocking; `NonInteractivePrompter` backs `--yes`/`--no-input`;
`ScriptedPrompter` is for unit tests. This seam is why prompts can be tested
without a TTY, and why nothing hangs in CI.

**Soft deletion is a `deleted_at` timestamp, not a magic category.** A deleted
task keeps its real `category_id` so restore is lossless. `live_tasks()` excludes
them; `get_deleted_tasks()` returns them. `list`/`search` must go through
`live_tasks()`.

**Each storage backend owns its own file** inside the `storage.path` *directory*
(`trtodo-data.json`, `trtodo-data.db`). They must never point at the same path.
Changing `storage.type` migrates data via `migrate_storage`, which only ever
reads the source and refuses to overwrite a non-empty destination.

**SQLite schema versioning is a guard rail, not a migration system.**
`SCHEMA_VERSION` is 2. `initialize_schema` reads the stored version before
touching any table and rejects anything newer. There is one hand-written v1→v2
step (adding `deleted_at`). Adding a real migration system is deferred — do not
mistake the existing code for one.

## Testing conventions

- Integration tests in `tests/` build and invoke the real binary. They **must**
  pass `--config` pointed at a `TempDir` so they never touch the real `$HOME`.
  There is no exception to this.
- Prompt-dependent paths are tested via `ScriptedPrompter` (unit) or by asserting
  the non-interactive error (integration) — never by piping `"y\n"` at a
  subprocess.
- Doc comments here are unusually dense and explain *why*, including why
  alternatives were rejected. Match that; it is the house style and it is load
  bearing, since most of this code encodes a decision someone will otherwise
  undo.

## Code comments

**No issue or PR numbers in comments.** Not `(issue #29)`, not `Issue #25:`,
not `issue #22's second half`. Code has to be readable by someone who does not
have the tracker open — which includes every session that starts cold.

Write the reason instead of a pointer to it. If a comment currently leans on an
issue number to carry its meaning ("the entire point of issue #29's design"),
that is a comment missing its actual content: say *what* the design is and why
("the entire point of soft deletion keeping the real `category_id`").

Backlog links belong in commit messages and pull requests, where `Closes #NN`
is expected and traceability already lives.

## Commit conventions

[Conventional Commits](https://www.conventionalcommits.org/), enforced by
commitlint in CI on pull requests (not on `main` — the older history predates
the convention). The PR title is linted too, because a squash-merge makes the
title the commit that lands on `main`.

```
feat(storage): migrate data when storage.type changes
fix(add): reject an unresolvable default-category
```

Bodies should explain the reasoning, and end with `Closes #NN` where applicable.

## State as of PR #34

PR #34 closes #17, #25, #27, #31, #32, #33. It was built as four parallel
branches and merged; the history was then linearized into five conventional
commits.

Open and deliberately not started:

- **#16 category ordering** — needs an `order` field honored across both
  backends plus a reorder command. `Category.order` already exists and is
  persisted, but nothing lets a user set it.
- **#19 split `config.rs`** — it does three jobs (key schema/validation,
  persistence, backend construction). Collides with anything else touching
  config, so do it on a quiet tree.
- **#20 / #21 tests and doc comments** — partly self-resolving as features land;
  worth re-scoping rather than treating "TEST ALL THE THINGS" as a spec.
- **#4** is effectively resolved by #27 and can likely be closed with a pointer.

## Rough edges and session cost

`docs/WORKING-NOTES.md` covers what this file deliberately leaves out: known
defects not yet filed (the binary is built as `trusty_rusty_todo_list`, not the
documented `trtodo`; renaming a category orphans `default-category`), build
facts that surprise people (`cargo test --lib` fails — there is no lib target;
`rusqlite` is bundled, so never delete `target/`), and the practices that keep
an AI-assisted session from spending its budget re-deriving known things.

Read it before parallelizing work across subagents or scoping a branch from an
issue — those are the two places this project has historically wasted the most
effort.
