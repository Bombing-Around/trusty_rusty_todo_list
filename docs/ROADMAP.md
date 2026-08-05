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

What that leaves is a CLI nobody can install. The data model no longer
carries fields the CLI never exposes — `description` is reachable as of #55,
and `due_date` is phase 2's whole subject. `Task.order` is the one field
still stored and untouched by any command, deliberately: see "Deliberately
not filed" below.

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

One thing now sits in front of it that did not before: the MSRV floor, below.
Both of that section's halves — the `rusqlite` code break and the toolchain
bump — are cheaper to do before there is a published install story than after.

Two things the release-engineering pass settled that are worth not
re-litigating:

- **The MSRV floor is 1.78 because of the lockfile, not the dependencies.**
  The highest floor any dependency declares is clap's 1.70, and it is
  unreachable: the committed `Cargo.lock` is lockfile v4, which no Cargo
  before 1.78 can parse. Lowering the floor means regenerating the lockfile
  in the older format, which is a deliberate choice and not a free one.

  **This has since expired — see "The MSRV floor" below.** It was true when
  written and is kept here because the reasoning still explains why the
  number is 1.78 rather than 1.70. It no longer explains why it should
  *stay* there.
- **The test count is reported, never enforced.** A hard failure on a
  decreasing count also fires on legitimate consolidation, and a check people
  learn to override is worse than no check. The direction of the number is the
  signal.
- **The crate is kept publishable, and CI packages it on every run.** crates.io
  is a possible destination rather than a settled one; `publish = false` would
  have blocked the dry run too, letting the manifest rot unpublishable with
  nothing to report it. One thing is still open and gets more expensive after
  a first publish, not less: the package name (`trusty_rusty_todo_list`) and
  the binary name (`trt`) differ, so installing would be
  `cargo install trusty_rusty_todo_list` to get a `trt`. They cannot be made
  to match by renaming the package — `trt` on crates.io belongs to an
  unrelated tokio runtime library. That crate ships no executable, so there is
  no PATH collision; the clash is only over the crates.io namespace.
  `trusty_rusty_todo_list` was unregistered when this was written, and saying
  so reserves nothing — publishing is the only thing that holds a name.

## The MSRV floor

The lockfile argument above no longer decides this. Dependencies now genuinely
require more than 1.78, and the question has changed from "what can the
lockfile parse" to "do we keep taking dependency updates at all".

What forced it was the `rusqlite` 0.30 → 0.40 bump, #66. That pull request
fails two independent ways, and they are worth keeping apart:

- **Code.** Five `E0277` errors on `Row::get`'s `FromSql` bound — `u64` is no
  longer an implementing type. Real API break across ten majors, fixable in
  one file, nothing to do with the toolchain. The `DateTime` columns are
  hand-parsed from `String` anyway, so whether the `chrono` feature still
  earns its place is worth asking at the same time.
- **Toolchain.** Something in the resulting tree uses edition 2024, which
  Cargo 1.78 cannot parse at all. That is a hard capability gate, not an
  advisory `rust-version` that can be ignored.

Two facts make this bigger than one dependency:

- **`rusqlite` declares no `rust-version`, in any version from 0.30 through
  0.40.** It never states its floor, so Cargo's MSRV-aware version selection
  — which only holds back crates that declare one — can never protect this
  project from it. The 1.78 floor was being enforced by the CI job and
  nothing else.
- **It is not only `rusqlite`.** The lockfile currently carries clap 4.5, and
  **clap 4.6.0 declares `rust-version = "1.85"`**. The next clap minor does
  the same thing. This is the ecosystem-wide edition-2024 wave, not one
  crate's choice.

So holding 1.78 means pinning clap, pinning `rusqlite`, and declining
dependency updates generally — a security-maintenance position, not merely a
compatibility one, on the crate that parses untrusted input.

**What raising it actually costs here is small**, and smaller than the same
decision would be for a library. There is no lib target, so there are no
downstream compilers to break — MSRV as a semver-adjacent promise does not
apply. The floor gates exactly one group: people building from source against
a system toolchain rather than rustup. That group nearly vanishes once #44
ships prebuilt binaries, which is the ordering argument: **raising the floor
before there is a published install story is cheap, and raising it after is
not.** Do this ahead of #44, not behind it.

**The policy, so this is not re-argued on every Dependabot pull request: the
MSRV is an output, not an input.** Declare whatever the dependencies actually
require, keep the CI job that proves the declared number is true, and treat a
raise as an ordinary consequence of a bump rather than an event needing its
own decision. This matches what was already decided about the test count —
reported, never enforced — and the reason the MSRV job exists at all, which
is to stop the number drifting into another claim nobody checks. The
alternative, a pinned floor that gets defended, is really a growing set of
pinned crates that also get defended.

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
| ~~#54~~ | **Landed.** `list` honours the category context via `has_explicit_category_context()`, gained `--category` and `--all`, and names the category it narrowed to. Filtering is one pass over `live_tasks()` in `TaskManager`. |
| ~~#55~~ | **Landed.** `description` is reachable on tasks and categories: set at creation, edited on `update`, blanked only by `--clear-description`, and shown by the new `trt show` and in `category list`. |
| ~~#56~~ | **Landed.** Eighteen blanket `#[allow(dead_code)]` markers removed, and the fourteen unreachable items behind them deleted. `src/` now has none; keep it that way — a blanket marker on an `impl` block is how the previous fourteen went unnoticed. |
| ~~#70~~ | **Landed.** `validate_storage_path` asks `faccessat`/`AT_EACCESS` whether the calling process can write, instead of reading the owner's mode bit — which passed `/` for every user and so rejected almost nothing. Windows remains a no-op, now documented as deliberate rather than a silent `cfg` gap. |

Three decisions these settled, each of which the issue left genuinely open:

- **`list` narrows silently, but says so.** The argument against — that a
  context set days ago hides tasks and loses one — is answered by the header
  naming the category, the same way `add` names the category it resolved.
  `--all` is the escape hatch alongside `category clear`.
- **`description` was exposed rather than removed.** Removal reads simpler
  and is not: dropping a column means a SQLite schema step, and
  `SCHEMA_VERSION` is a guard rail with one hand-written migration behind it,
  not a migration system.
- **Blanking a description needs `--clear-description`.** An empty
  `--description ""` deliberately does not do it, so a description is never
  lost by accident. Making that work required `update --to` and
  `category update <new_name>` to become optional, so an edit that changes
  only a description does not have to rename anything; both now refuse an
  invocation that names no field at all rather than printing a misleading
  "updated".

Two follow-ups these left behind, neither filed yet:

- **`list --category` has no `-c` short form**, because `-c` on `list` was
  already `--completed`. Every other task command spells category `-c`, so
  `trt list -c Work` does not do what muscle memory expects.
- **`ConfigManager::stored_config` panics on any config read error**
  (`self.storage.load().unwrap()`). Pointing `--config` at a directory
  produces a Rust backtrace instead of an error message. Predates all of the
  above.

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
