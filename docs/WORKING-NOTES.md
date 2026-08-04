# Working notes

Two things that don't belong in `CLAUDE.md` because they aren't needed on
every turn: the rough edges worth knowing before you touch something, and the
practices that keep an AI-assisted session from burning budget re-deriving
what is already known.

`CLAUDE.md` holds the invariants — the rules you must not break. This file
holds the friction: what is currently wrong, what will waste your time, and
how to spend fewer tokens getting the same work done.

---

## Rough edges

### Build and test facts that surprise people

- **There is no lib target.** `cargo test --lib` fails outright with `no
  library targets found in package`. The unit tests live in the binary
  target alongside the integration tests. Use plain `cargo test`, or
  `cargo test --bin trtodo` to skip the integration suites.

- **`rusqlite` is vendored with `features = ["bundled"]`**, so a cold build
  compiles SQLite from C source and takes minutes. A warm build takes
  seconds. Never delete `target/` to "start clean" — it is the single most
  expensive thing you can do to a session, and it fixes nothing that
  `cargo clean -p trusty_rusty_todo_list` wouldn't fix faster.

- **`cargo clippy -- -D warnings` is stricter than bare `cargo clippy`.**
  CI runs the strict form. A bare clippy run that looks clean proves
  nothing about whether CI will pass.

- **CI reports the test count, per binary and total, in every run's job
  summary.** If that total drops, something was deleted rather than fixed —
  that is the one thing the number is for, and it is the direction of the
  number that matters, not the value. It is deliberately not a coverage
  target and deliberately not a hard CI failure; a build that breaks on a
  decreasing count also breaks on legitimate consolidation, and then people
  learn to override it.

  The count is not repeated here on purpose. It used to be, and it drifted
  into two different stale numbers in two documents — which is worse than no
  number, because a written one gets quoted instead of re-derived. Read it
  off the latest CI run, or `cargo test` locally.

- **A shared `CARGO_TARGET_DIR` across worktrees will lie to you.** Sharing one
  target directory between parallel worktrees looks like an easy way to avoid
  paying the bundled-`rusqlite` cold build more than once. It is not: every
  worktree writes the same `debug/trtodo`, and the integration tests invoke
  exactly that binary. Two agents building concurrently means one runs its
  tests against the other's binary, and the failures that produces are
  spurious and unreproducible. Give each worktree its own target directory and
  pay the build, or build them one at a time.

---

## Keeping sessions cheap

Ordered by how much each one actually cost or saved when this code was
being worked on — not by how good the advice sounds.

### 1. Don't fan out subagents for work that isn't big and genuinely parallel

The single largest avoidable cost. Every subagent starts cold: it re-reads
the same files, re-derives the same invariants, and re-discovers the same
constraints the parent already knows. Four agents on four issues meant
paying the discovery tax four times for one codebase.

It is worth it when the tasks are large, touch disjoint files, and would
serialize badly otherwise. It is *not* worth it for anything you could do
inline in a few tool calls. A task described as "thorough" or having
"several parts" is not by itself a reason to spawn anything.

### 2. Isolated agents cannot see integration bugs — budget for the merge

Four features that each pass their own tests can still combine into a
failure none of them could see. That happened here: making `--category`
optional turned Uncategorized into the default landing place for `add`,
which walked straight into a SQLite foreign-key constraint that only
existed because Uncategorized is synthesized rather than stored. Every
individual branch was green.

If you parallelize, reserve real budget for a combined smoke test that
exercises the features *against each other*, not just a merge that compiles.

### 3. Check what an agent ran, not what it says it did

A subagent reporting "all tests pass" is a claim about whatever command it
actually ran. One reported success here having run only `cargo test --bin
trtodo`, which skips all seven integration suites — the exact place this
project's regressions surface. Another reported a passing suite whose numbers
came from a target directory a concurrent build was overwriting.

Neither agent was being careless in a way its own transcript would reveal.
Re-run the full trio yourself on the branch before opening a pull request; it
is seconds against a warm build, and it is the only number worth quoting.

### 4. Check the issue against the code before scoping work from it

Issue text goes stale. One issue here asserted that `SCHEMA_VERSION` was
"a hardcoded 1" and proposed deleting the version table on that basis; the
constant was actually `2`, with a real v1→v2 migration behind it. Acting on
the issue as written would have destroyed history.

One `grep` before scoping is cheaper than one wrong branch.

### 5. Decide the commit convention before branching, not after

Retrofitting Conventional Commits onto four already-merged branches meant
cherry-picking, re-resolving conflicts that had already been resolved once,
and verifying by tree hash that the rewritten history still produced
identical output. That verification was worth doing — it caught a
duplicated README row — but the whole exercise was avoidable by deciding
the format up front.

### 6. Read ranges, not whole files

Four files here are 700–1100 lines. Reading one end to end to change a
comment near the bottom costs the whole file every time. `grep -n` for the
anchor, then read the range around it.

Conversely: don't re-read a file immediately after editing it to "check"
the edit. A failed edit reports itself.

### 7. Run the full CI trio once, at the end

`cargo fmt --all -- --check`, `cargo clippy -- -D warnings`, `cargo test`.
Running all three after every small edit triples the cost of iterating for
no added signal. Run `cargo check` while working; run the trio before
pushing.

### 8. Let `CLAUDE.md` do its job

It is loaded automatically at session start. That is precisely why it must
stay short and stay true: everything in it is paid for on every single
session, so a stale line there is worse than a stale line anywhere else in
the repo. If something in it is wrong, fixing it is high-value work, not
housekeeping.

Anything that is *not* needed every turn — including everything in this
file — belongs somewhere `CLAUDE.md` merely points at.
