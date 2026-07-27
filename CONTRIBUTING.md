<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# Contributing

en | [zh_CN](CONTRIBUTING.zh_CN.md)

> [!WARNING]
>
> Early development: code has **never been human-reviewed**; the CLI surface may
> change frequently.

> [!NOTE]
>
> Translated from the Chinese version; may contain errors.

Conventions for contributing to `busx`.

## Table of contents

- [Clone](#clone)
- [Build & run](#build--run)
- [Testing](#testing)
- [Code style](#code-style)
- [Documentation](#documentation)
- [Commit messages](#commit-messages)
- [AI-assisted changes](#ai-assisted-changes)
- [License & REUSE](#license--reuse)

## Clone

A `.format` submodule ships shared `EditorConfig` / `prettier` config, so clone
with `--recurse-submodules`:

```bash
git clone --recurse-submodules https://github.com/black-desk/busx.git
```

## Build & run

Cargo workspace (root `Cargo.toml` is a virtual manifest); code lives in
`crates/busx/`, test fixtures in `crates/testbus/`. Run all `cargo` commands
from the **repository root**:

```bash
cargo build
cargo run -- <args>               # e.g. cargo run -- list
cargo run -- --help               # show the CLI
cargo run                         # enter the TUI
```

## Testing

**Testing principle: only do end-to-end testing at the public-interface layer;
do not write unit tests for internal modules.**

`busx` is a pure binary crate (the root `Cargo.toml` has no lib target), so it
exposes only two public surfaces: CLI subcommand input / output, and TUI
interaction / rendering. Every test enters through one of them —
`crates/busx/tests/` uses `assert_cmd` to spawn the real `busx` binary; TUI
tests run the real `busx` as a subprocess inside a PTY where `tuiprobe` feeds
keyboard / mouse events and reads the rendered output. Internal modules
(`dbus/`, `value/`, `ops/`, …) get no standalone unit tests; their behavior is
verified indirectly through the public surface. **If a test needs `pub(crate)`
or narrower visibility, it is in the wrong place** — move it into `tests/` and
verify via a CLI / TUI output frame.

Tests split into two layers: **integration tests** (`crates/busx/tests/`) and
**TUI snapshot tests** (`crates/busx/tests/tui_pty.rs`, via `tuiprobe`), both
driven by the `testbus` fixture.

### The testbus fixture

`testbus::bus_owned()` spins up a standalone `dbus-daemon` in the background,
registers a test service `org.busx.Test`, and returns its address. Integration
tests point `busx` at it via `--address`, so they **do not depend on the system
/ session bus state** and stay deterministic:

```rust
let addr = testbus::bus_owned().address.clone();
Command::cargo_bin("busx")?
    .args(["--address", &addr, "list"])
    ...
```

A typical usage lives in `crates/busx/tests/list.rs`.

### Snapshot tests (insta)

TUI rendering logic is snapshot-tested with [`insta`][insta]; the expected
snapshots live in `crates/busx/tests/snapshots/*.snap`. Each test launches the
real `busx` (on the `testbus` private bus) inside a real terminal from
`tuiprobe`, drives it to the target state, then snapshots the screen —
exercising the full `main` → CLI → crossterm → ratatui → `render` pipeline.
`wait_for_snapshot!` polls until the screen matches an existing expected
snapshot (intermediate frames are never persisted), then
`insta::assert_snapshot!` asserts the new frame.

[insta]: https://insta.rs

When you **intentionally** change TUI rendering (layout, copy, colors), the
snapshot tests fail — expected. Accept the new output:

```bash
cargo install cargo-insta          # one-time install
cargo insta accept                 # accept all new snapshots
cargo insta review                 # review one by one, safer
```

Never commit snapshot changes you did not intend; if it is an unintended
regression, check `render.rs` / `update.rs` instead of blindly accepting.

CI reports test coverage to [codecov][codecov] via
[`black-desk/workflows/rust`][wf-rust]. Add test cases relevant to your changes.

[wf-rust]: https://github.com/black-desk/workflows
[codecov]: https://codecov.io/github/black-desk/busx

## Code style

Style is fixed by the `.format` submodule:

- **Rust**: 4-space indent (see `[*.rs]` in `.format/.editorconfig`).

  ```bash
  cargo fmt --all
  cargo clippy --all-targets --all-features -- -D warnings
  ```

- **Markdown / YAML / JSON / TOML**: 2-space indent, hard wrap at 80 columns
  (`printWidth: 80, proseWrap: "always"`, see `.format/.prettierrc`). After
  editing docs, run prettier (or use prettier-on-save):

  ```bash
  npx prettier --write '**/*.md'
  ```

- **Shell scripts**: tab indent, `#!/usr/bin/env bash`, `set -euo pipefail`.
  Follow the style of the existing scripts under `scripts/`.

## Documentation

**Project docs should only contain project-specific knowledge.**

`README.md`, `CONTRIBUTING.md`, and future `docs/` never cover generic tool
usage (how to run `cargo test`, git, Rust syntax) — readers can look that up in
the official docs. The bar: **if a sentence still holds for a different project,
it does not belong here.** What belongs: this project's own commands,
conventions, design decisions, CLI / TUI behavior, build / CI config.

When editing docs, prune against this standard — no generic tutorials.

**Keep the docs in sync.** `README.md` and `CONTRIBUTING.md` must always reflect
the current state. When a change alters CLI / TUI behavior, build / CI config,
or any documented convention, update the affected doc in the same PR.

## Commit messages

Follow [Conventional Commits][cc]; the first line of a commit message has the
format:

```
<type>(<scope>): <subject>
```

CI lints commit messages with [commitlint][cl]
(`@commitlint/config-conventional`, the `generic` job in
`.github/workflows/ci.yaml`). Hard rule: **body lines hard-wrap at 100 columns**
(`body-max-line-length`); subject and footer share that limit, so a body line
over 100 columns fails `generic`.

[cc]: https://www.conventionalcommits.org/en/v1.0.0/
[cl]: https://commitlint.js.org/

## AI-assisted changes

Any change made with an AI tool MUST be attributed.

### Attribution — `Assisted-by`

Add an `Assisted-by` trailer to the commit message, per the Linux kernel's
[coding assistants][kernel-ca] guidance (see also the "Using Assisted-by"
section of [submitting-patches][kernel-sp]).

The required format:

```
Assisted-by: AGENT:MODEL
```

- `AGENT` — the AI tool or framework, e.g. `Codex`, `Claude`.
- `MODEL` — the model version, e.g. `gpt-5`, `claude-3-opus`.

Optional specialized analysis tools may follow (basic tools like git / cargo /
editors are never listed):

```
Assisted-by: Codex:gpt-5
Assisted-by: Claude:claude-3-opus sparse
```

If you cannot reliably determine your own `AGENT` / `MODEL`, do not guess — ask
the user and record exactly what they tell you.

[kernel-ca]:
  https://www.kernel.org/doc/html/latest/process/coding-assistants.html
[kernel-sp]:
  https://www.kernel.org/doc/html/latest/process/submitting-patches.html

## License & REUSE

Code is **GPL-3.0-or-later**; docs / config / scripts are **MIT** (see `LICENSE`
and `LICENSES/`). The repo follows the [REUSE spec][reuse], and **every new /
modified file must carry an SPDX header**:

- Rust / Shell / TOML / YAML files use a comment header:
  ```rust
  // SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
  //
  // SPDX-License-Identifier: GPL-3.0-or-later
  ```
  (docs use `MIT`; scripts depend on their content — existing scripts mostly use
  `GPL-3.0-or-later`.)
- Markdown files use an HTML comment header (see the top of this file).

Use the **current year**. For a new substantive change to an existing file,
**append a line** below the existing `SPDX-FileCopyrightText` — do not overwrite
the original year.

```bash
reuse lint
```

The `format` CI job runs `reuse lint`; a missing / incorrect header fails the
PR.

[reuse]: https://reuse.software/spec-3.3/
