# Roadmap

Where this project goes after phase 1, and why in that order.

The README names four phases and has since the first commit. This file is the
bridge between those sentences and the issues that implement them: what is
actually left, what has to be decided before it can be built, and what is
deliberately not being worked on yet.

Issue #43 is the tracking issue and holds the same list as sub-issues. When
the two disagree, the issues win — this file is a map, not a database.

---

## Where phase 1 landed

Phase 1 as the README describes it — tasks, categories, centralized config,
JSON storage, "ultimately SQLite" — is done. Both backends exist and migrate
between each other, categories have a persistent context and an explicit
order, deletion is soft and reversible, config has defaults, validation, and a
first-run offer.

What that leaves is a CLI nobody can install and a data model carrying fields
the CLI never exposes.

---

## Release engineering

First, because none of the rest reaches a user otherwise, and because the
decisions it forces — supported platforms, MSRV, package identity — get more
expensive the longer there is code depending on the answer.

| Issue | What |
| --- | --- |
| ~~#45~~ | **Landed.** Package metadata, `license = "GPL-3.0-only"` matching the tree, and `rust-version = "1.78"` — the floor the suite is verified against, with a CI job that keeps it true. Left publishable, with `cargo publish --dry-run` in CI. |
| ~~#46~~ | **Landed.** `test` is a matrix across Linux, macOS, and Windows; `fmt`/`clippy` split into a job that runs once. Dependabot enabled, monthly and grouped. |
| ~~#47~~ | **Landed.** `tempfile` is a dev-dependency only. |
| #44 | The release workflow itself: tag-triggered cross-platform binaries, checksums, generated notes, a real install section. |
| ~~#48~~ | **Landed.** CI derives and prints per-binary and total test counts; the hand-maintained number is gone from the working notes. |

#44 comes last of these on purpose: building binaries for platforms the suite
has never run on, from a manifest that does not say what license they are
under, is publishing guesses. Its prerequisites are now all in place — what it
is waiting on is a decision about what a release contains, not more groundwork.

Two things the release-engineering pass settled that are worth not
re-litigating:

- **The MSRV floor is 1.78 because of the lockfile, not the dependencies.**
  The highest floor any dependency declares is clap's 1.70, and it is
  unreachable: the committed `Cargo.lock` is lockfile v4, which no Cargo
  before 1.78 can parse. Lowering the floor means regenerating the lockfile
  in the older format, which is a deliberate choice and not a free one.
- **The test count is reported, never enforced.** A hard failure on a
  decreasing count also fires on legitimate consolidation, and a check people
  learn to override is worse than no check. The direction of the number is the
  signal.
- **The crate is kept publishable, and CI packages it on every run.** crates.io
  is a possible destination rather than a settled one; `publish = false` would
  have blocked the dry run too, letting the manifest rot unpublishable with
  nothing to report it. One thing is still open and gets more expensive after
  a first publish, not less: the package name (`trusty_rusty_todo_list`) and
  the binary name (`trtodo`) differ, so installing would be
  `cargo install trusty_rusty_todo_list` to get a `trtodo`. Both names were
  unregistered when this was written, and neither is reserved by saying so —
  publishing is the only thing that holds a name.

## Phase 2 — dates and times

Further along than it looks. `Task::due_date` is a real, fully persisted
field: a `due_date TEXT` column in the SQLite schema, serialized in JSON,
round-tripped on load, with a setter. The storage half is paid for. The user
half does not exist — nothing sets a due date, nothing shows one, nothing
filters on one.

| Issue | What |
| --- | --- |
| #49 | Decide the input formats and the display timezone. A decision, not code. |
| #50 | Expose due dates: set at creation, change, clear, show in `list`, mark overdue. |
| #51 | Filter and sort by due date, and decide `list`'s sort order explicitly. |

#49 leads because every later piece encodes its answer, and because the one
timestamp the CLI displays today deliberately punted on the local-time
question until something forced it. Due dates are that something.

## Phase 3 — reminders

The README's "scheduler / cron / etc." phase. The honest shape for a CLI is
not a daemon: it is a command that is pleasant to put in a crontab, letting
the scheduler and the notifier be whatever the user already runs.

| Issue | What |
| --- | --- |
| #52 | Machine-readable output (`--format json`). A scheduler needs something to parse, and today every command prints prose whose wording has already changed once. |
| #53 | The command a scheduler calls: due/overdue tasks, a real exit-code contract, silent when there is nothing to say, never prompting. |

## Phase 4 — sync

Not scoped, and deliberately without issues. It needs a conflict model before
it needs code; filing implementation issues now would be filing guesses.
Revisit once phase 3 ships.

---

## Rough edges

Found reviewing the tree against the README rather than against the backlog.
None blocks a phase. They are on the roadmap because each one is the kind of
thing that stops being cheap to change once there is a release with users
behind it.

| Issue | What |
| --- | --- |
| #54 | `list` ignores the category context and cannot be scoped to a category, while every other task command honours it. |
| #55 | `description` is persisted on tasks and categories, threaded through the manager APIs, and can never be set. |
| ~~#56~~ | **Landed.** Eighteen blanket `#[allow(dead_code)]` markers removed, and the fourteen unreachable items behind them deleted. `src/` now has none; keep it that way — a blanket marker on an `impl` block is how the previous fourteen went unnoticed. |

#56 shifted #54 slightly: the two unused storage combinators it cited as
evidence are gone, so `list --category` starts from a smaller surface rather
than an existing-but-unused one.

## Standing backlog

#19 (split `config.rs`), #20 (test coverage gaps), and #21 (inline docs) predate
this roadmap and are not duplicated into it. #20's backend-parity bullet and
#46 overlap deliberately: one is the tests, the other is somewhere to run them.

---

## Deliberately not filed

- **Sync**, above.
- **Recurring tasks.** Plausible once due dates exist. A repeat rule interacts
  with soft deletion and completion in ways worth designing against real
  due-date usage rather than in the abstract.
- **Task ordering.** Categories have explicit order; `Task.order` exists and no
  command touches it. #56 kept the field deliberately — dropping it is a schema
  change to remove a column that costs nothing — and deleted its unused setter.
  Whether it becomes a feature is still open, and #51 is the natural moment to
  answer it, since defining `list`'s sort order forces the question.
