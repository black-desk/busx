<!--
SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# Contributing

en | [zh_CN](CONTRIBUTING.zh_CN.md)

> [!WARNING]
>
> This project is in an **early development stage**; its code has **never been
> reviewed by a human**, and the CLI surface may change frequently.

> [!NOTE]
>
> This English CONTRIBUTING is translated from the Chinese version and may
> contain errors.

Contributions to `busx` are very welcome — whether you install `busx` to use it,
fix a bug, add a test, or build a new feature. This document lists the
conventions you need to know to contribute.

## Table of contents

- [Clone](#clone)
- [Build & run](#build--run)
- [Testing](#testing)
- [Code style](#code-style)
- [Documentation](#documentation)
- [Commit messages](#commit-messages)
- [Pull request workflow](#pull-request-workflow)
- [License & REUSE](#license--reuse)

## Clone

The repository ships a `.format` submodule (shared `EditorConfig` / `prettier` /
formatting config), so always clone with `--recurse-submodules`:

```bash
git clone --recurse-submodules https://github.com/black-desk/busx.git
```

If you already cloned it without submodules, catch up:

```bash
git submodule update --init --recursive
```

## Build & run

`busx` is a Cargo workspace (the root `Cargo.toml` is a virtual manifest); the
actual code lives in `crates/busx/` and the test fixtures live in
`crates/testbus/`. All `cargo` commands can be run straight from the
**repository root**:

```bash
cargo build                       # debug build
cargo run -- <args>               # run directly, e.g. cargo run -- list
cargo run -- --help               # show the CLI
cargo run                         # enter the TUI
```

## Testing

**Testing principle: only do end-to-end testing at the public-interface layer;
do not write unit tests for internal modules.**

`busx` is a pure binary crate (the root `Cargo.toml` has no lib target), so it
exposes only two public surfaces: the input / output of the CLI subcommands, and
the interaction / rendering of the TUI. Every test should enter through one of
these two layers — `crates/busx/tests/` uses `assert_cmd` to spawn a subprocess
running the real `busx` binary; TUI tests run the real `busx` as a subprocess
inside a pseudo-terminal (PTY), where `tuiprobe` feeds keyboard / mouse events
and reads the rendered output. Internal modules (private functions in `dbus/`,
`value/`, `ops/`, etc.) should not have standalone unit tests; their behavior
should be verifiable indirectly through the public surface. In other words: **if
a test needs `pub(crate)` or narrower visibility to be written, it is in the
wrong place** — move the assertion into `tests/` and verify it through a `busx`
command line / a TUI output frame.

Following this principle, tests are split into two layers: **integration tests**
(`crates/busx/tests/`) and **TUI snapshot tests**
(`crates/busx/tests/tui_pty.rs`, via `tuiprobe`), both driven by the `testbus`
fixture.

> Historical note: earlier versions used a ratatui `TestBackend` with in-process
> scripts driving `State` → `render` snapshot tests
> (`crates/busx/src/tui/snapshot_tests.rs`); this was rewritten to PTY
> end-to-end snapshots (see below) in commit `818ce2af`, and that file was
> deleted.

### The testbus fixture

`testbus::bus_owned()` spins up a standalone `dbus-daemon` in the background,
registers a test service named `org.busx.Test`, and returns its address.
Integration tests point `busx` at this bus via `--address`, so they **do not
depend on the system / session bus state** and are deterministic:

```rust
let addr = testbus::bus_owned().address.clone();
Command::cargo_bin("busx")?
    .args(["--address", &addr, "list"])
    ...
```

A typical usage lives in `crates/busx/tests/list.rs`.

### Snapshot tests (insta)

TUI rendering logic is snapshot-tested with [`insta`][insta]; the golden
snapshots live in `crates/busx/tests/snapshots/*.snap`. Each test launches the
real `busx` (connected to the `testbus` private bus) inside a real terminal
spawned by `tuiprobe`, feeds it keyboard / mouse events to drive it to the
target state, then snapshots the terminal screen — exercising the full `main` →
CLI → crossterm → ratatui → `render` pipeline. `wait_for_snapshot!` polls until
the screen matches an existing golden snapshot (intermediate frames before
convergence are never persisted), then `insta::assert_snapshot!` asserts the new
frame.

[insta]: https://insta.rs

When you **intentionally** change the TUI rendering (e.g. tweaking layout, copy,
or colors), the snapshot tests will fail. This is expected — accept the new
output with `cargo insta`:

```bash
cargo install cargo-insta          # one-time install
cargo insta accept                 # accept all new snapshots
cargo insta review                 # review one by one, safer
```

Do not commit snapshot changes you **did not intend** to accept; if it is an
unintended regression, first go back and check `render.rs` / `update.rs` instead
of blindly accepting to hide the problem.

### Coverage

CI reports to [codecov][codecov] via [`black-desk/workflows/rust`][wf-rust]. You
do not need to run coverage locally, but please try to add tests when you change
core paths (`dbus/`, `value/`, `tui/update.rs`).

[wf-rust]: https://github.com/black-desk/workflows
[codecov]: https://codecov.io/github/black-desk/busx

## Code style

Style is fixed by the `.format` submodule, not by word of mouth:

- **Rust**: 4-space indent (see `[*.rs]` in `.format/.editorconfig`).

  ```bash
  cargo fmt --all
  cargo clippy --all-targets --all-features -- -D warnings
  ```

- **Markdown / YAML / JSON / TOML**: 2-space indent, hard wrap at 80 columns
  (`printWidth: 80, proseWrap: "always"`, see `.format/.prettierrc`). After
  editing docs, run prettier once (or just use your editor's prettier-on-save):

  ```bash
  npx prettier --write '**/*.md'
  ```

- **Shell scripts**: tab indent, `#!/usr/bin/env bash`, `set -euo pipefail`.
  Follow the style of the existing scripts under `scripts/`.

## Documentation

**Project docs should only contain project-specific knowledge.**

`README.md`, `CONTRIBUTING.md`, and any future `docs/` all follow this standard:
generic tool usage (how to run `cargo test`, how to use `git`, what Rust syntax
is) is never written — readers can look that up in the official docs themselves.
The bar is simple: **if a sentence still holds when you swap to a different
project, it should not be here**. What belongs here is: this project's own
commands, conventions, design decisions, CLI / TUI behavior, build / CI
configuration, and so on.

When adding / editing docs, prune and weigh against this standard — do not stuff
in generic tutorials.

## Commit messages

Follow [Conventional Commits][cc]; the first line of a commit message has the
format:

```
<type>(<scope>): <subject>
```

- **`type`** — one of:
  - `feat` — a new feature (CLI subcommand, TUI interaction, new output format,
    etc.)
  - `fix` — a bug fix
  - `refactor` — a refactor with no behavior change
  - `docs` — documentation changes
  - `test` — test-only changes
  - `style` — formatting / indentation / whitespace
  - `perf` — a performance improvement
  - `ci` / `chore` — CI / build / deps (`chore(deps)` for dependabot bumps)
- **`scope`** (optional but recommended): the affected module, e.g. `tui`,
  `cli`, `complete`, `dbus`, `value`, `ops`, `monitor`, `list`, `cd`, `ci`.
- **`subject`**: imperative mood, present tense, lowercase first word, no
  trailing period.

For example:

```
feat(tui): add / inline filter to list screens
fix(monitor): respect --timeout on idle bus
refactor(value): share sig_str/access_str helpers
docs(readme): update --help blocks for -v and --log
chore(deps): bump clap from 4.6.1 to 4.6.2
```

See the full history with `git log --oneline`. For commits touching a GitHub
issue / PR, add `Closes #42` or `Refs #42` in the body at the end.

[cc]: https://www.conventionalcommits.org/en/v1.0.0/

## Pull request workflow

1. Branch off `master`:
   ```bash
   git checkout -b feat/my-feature master
   ```
2. One PR does one thing. PRs that mix several unrelated changes are painful to
   review and will usually be asked to be split up.
3. Run the core checks locally:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test
   npx prettier --check '**/*.md'   # only if you touched docs
   ```
4. **Add tests.** New features get an integration test; TUI rendering changes
   update the snapshots; bug fixes get a regression test. If you are not sure
   how to write one, look at the existing tests under `crates/busx/tests/` or
   just ask in the PR.
5. In the PR description, explain **what you did / why / how you tested it**. If
   there is a related issue, add `Closes #N`.
6. Wait for review once CI is fully green. The `pass` job is the CI master
   switch and must be green.

## License & REUSE

The code in this repository is open source under **GPL-3.0-or-later**;
documentation / config / scripts are open source under **MIT** (see `LICENSE`
and `LICENSES/`). The repository follows the [REUSE specification][reuse], and
**every new / modified file must carry an SPDX header**:

- Rust / Shell / TOML / YAML files use a comment header:
  ```rust
  // SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>
  //
  // SPDX-License-Identifier: GPL-3.0-or-later
  ```
  (docs use `MIT`; scripts depend on their content — existing scripts mostly use
  `GPL-3.0-or-later`.)
- Markdown files use an HTML comment header (see the top of this file).

Use the **current year**; if you are adding a new substantive change to an
existing file, you may **append a new line** below the existing
`SPDX-FileCopyrightText` line — do not overwrite the original year.

```bash
reuse lint
```

The `format` job in CI runs `reuse lint`; a missing / incorrect header will fail
the PR.

[reuse]: https://reuse.software/spec-3.3/
