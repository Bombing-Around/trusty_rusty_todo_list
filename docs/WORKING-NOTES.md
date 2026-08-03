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

### The binary is not called `trtodo`

`README.md` line 21 says "The binary named `trtodo`", and every row of the
command table spells commands as `trtodo ...`. But `Cargo.toml` has no
`[[bin]]` section, so the binary takes the package name: `cargo build`
produces `target/debug/trusty_rusty_todo_list`.

Nothing is broken by this today — the integration tests find the binary
through `env!("CARGO_BIN_EXE_trusty_rusty_todo_list")`, which cargo generates
from the real target name — but the documented install does not produce the
documented command. By this repo's own rule (the README is the spec), this is
a bug in `Cargo.toml`, not in the README.

Filed as #35. The fix is one stanza plus a mechanical rename:

```toml
[[bin]]
name = "trtodo"
path = "src/main.rs"
```

...and then `CARGO_BIN_EXE_trusty_rusty_todo_list` → `CARGO_BIN_EXE_trtodo`
in all seven files under `tests/`. Deliberately not done yet: it is a real
change with a blast radius, and it deserves its own commit rather than
riding along with unrelated work.

### Two further gaps

- **Renaming a category orphans a `default-category` that names it** (#36). The
  setting stores a name on purpose (IDs are handed back out after a delete,
  so a stored ID can silently start pointing at an unrelated category). The
  consequence is that `category update Work Werk` leaves
  `default-category=Work` dangling, and the next bare `add` fails to resolve
  it. Renaming should probably follow the setting across.

- **`JsonStorage::save` is a bare `fs::write` with no format check** (#37). Not
  reachable through configuration now that each backend owns a distinct
  filename, but a direct constructor call could still have JSON clobber a
  SQLite file. The distinct filenames are what make this unreachable — see
  the warning in `CLAUDE.md` about not "cleaning up" that arrangement.

### Build and test facts that surprise people

- **There is no lib target.** `cargo test --lib` fails outright with `no
  library targets found in package`. The ~99 unit tests live in the binary
  target alongside the 50 integration tests. Use plain `cargo test`, or
  `cargo test --bin trusty_rusty_todo_list` to skip the integration suites.

- **`rusqlite` is vendored with `features = ["bundled"]`**, so a cold build
  compiles SQLite from C source and takes minutes. A warm build takes
  seconds. Never delete `target/` to "start clean" — it is the single most
  expensive thing you can do to a session, and it fixes nothing that
  `cargo clean -p trusty_rusty_todo_list` wouldn't fix faster.

- **`cargo clippy -- -D warnings` is stricter than bare `cargo clippy`.**
  CI runs the strict form. A bare clippy run that looks clean proves
  nothing about whether CI will pass.

- **Current state:** 149 tests across 7 binaries (99 unit + 3 + 3 + 14 + 7
  + 6 + 17). If that total drops, something was deleted rather than fixed.

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

### 3. Check the issue against the code before scoping work from it

Issue text goes stale. One issue here asserted that `SCHEMA_VERSION` was
"a hardcoded 1" and proposed deleting the version table on that basis; the
constant was actually `2`, with a real v1→v2 migration behind it. Acting on
the issue as written would have destroyed history.

One `grep` before scoping is cheaper than one wrong branch.

### 4. Decide the commit convention before branching, not after

Retrofitting Conventional Commits onto four already-merged branches meant
cherry-picking, re-resolving conflicts that had already been resolved once,
and verifying by tree hash that the rewritten history still produced
identical output. That verification was worth doing — it caught a
duplicated README row — but the whole exercise was avoidable by deciding
the format up front.

### 5. Read ranges, not whole files

Four files here are 700–1100 lines. Reading one end to end to change a
comment near the bottom costs the whole file every time. `grep -n` for the
anchor, then read the range around it.

Conversely: don't re-read a file immediately after editing it to "check"
the edit. A failed edit reports itself.

### 6. Run the full CI trio once, at the end

`cargo fmt --all -- --check`, `cargo clippy -- -D warnings`, `cargo test`.
Running all three after every small edit triples the cost of iterating for
no added signal. Run `cargo check` while working; run the trio before
pushing.

### 7. Let `CLAUDE.md` do its job

It is loaded automatically at session start. That is precisely why it must
stay short and stay true: everything in it is paid for on every single
session, so a stale line there is worse than a stale line anywhere else in
the repo. If something in it is wrong, fixing it is high-value work, not
housekeeping.

Anything that is *not* needed every turn — including everything in this
file — belongs somewhere `CLAUDE.md` merely points at.
