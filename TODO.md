<!--
SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# TODO

Known issues found during a snapshot / CLI-output review that may surprise or
confuse users. These were observed via the existing end-to-end tests and manual
`--help` output; none are fixed yet. (The copy-as popup right-border issue
originally listed here turned out to be a snapshot-filter regex bug, not a real
rendering defect, and has been fixed.)

## TUI

### Listen leaves the Result screen empty while armed

After arming a listen, the Result screen body is completely blank until the
first matching message arrives. `render_result` builds its body lines from
`r.messages` (or `r.result`); when the listen is armed but no message has
streamed yet, both are empty and nothing is drawn. There is no "listening…"
placeholder, so the screen reads as frozen / unresponsive.

- Reproduced by: `tui_pty__result_listen_method_80x20.snap` and
  `tui_pty__result_listen_property_80x20.snap`.
- Contrast: the CLI `monitor` command prints a `busx: monitoring (...)` line the
  moment it goes live; the TUI has no equivalent affordance.
- Suspected location: `crates/busx/src/tui/render.rs` (`render_result`, the
  `r.messages.is_empty()` / `r.loading` branches).

## CLI

### Subcommand positionals and most `monitor` flags have no help text

Every subcommand's positional arguments (`SERVICE`, `OBJECT`, `INTERFACE`,
`METHOD`, `SIGNATURE`, `ARGS`, `PROPERTY`, `VALUE`, `MEMBER`) show a blank
description in `<subcommand> --help`. On `monitor`, `[SERVICES]`, `--interface`,
`--member`, `--path`, `--sender`, `--match`, `--limit-messages`, and `--timeout`
are also blank (only `--type` carries a description). A new user cannot infer
argument order or meaning from the help.

- Reproduced by: `busx <subcommand> --help` for every subcommand.
- Suspected location: `crates/busx/src/cli.rs` (missing `help = "..."` /
  `long_help = "..."` on the `#[arg(...)]` fields).
