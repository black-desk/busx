<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>

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
- Contrast: the CLI `monitor` command prints a `busx: monitoring` line the
  moment it goes live; the TUI has no equivalent affordance.
- Suspected location: `crates/busx/src/tui/render.rs` (`render_result`, the
  `r.messages.is_empty()` / `r.loading` branches).

### Method-listen ready-gate has no timeout / fallback

When a listen uses BecomeMonitor, `app.rs` waits for the daemon's
`NameLost(own_name)` signal (the one that confirms monitor mode is live) and
discards every message via `continue` until it arrives (`monitor_ready`). If
that confirming signal is delivered late or never (daemon- and rule-dependent),
the gate stays shut: the listen appears armed but no message is ever shown, with
no timeout, error, or fallback. The content filter `is_become_monitor_noise`
(used on the CLI side) is the safer path — real traffic that races the
transition is never dropped — but the TUI path does not use it.

- Suspected location: `crates/busx/src/tui/app.rs` (the `monitor_ready` loop),
  `crates/busx/src/dbus/monitor.rs` (`is_monitor_ready_signal` /
  `is_become_monitor_noise` — the latter's doc already warns about the gate).

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

## Testing

### tuiprobe README API table is incomplete

The API table in `crates/tuiprobe/README.md` lists `screen_contents()` /
`contains()` but omits several public methods that exist and are used by the
busx test suite: `wait_for_rect`, `wait_for_rect_with_timeout`,
`screen_contents_crop`, and the `TuiProbeBuilder` (`probe.builder().cols()…`). A
test author reading only the README would miss them.

- Suspected location: `crates/tuiprobe/README.md` (the "Output" / builder
  tables), vs. the actual surface in `crates/tuiprobe/src/harness.rs`.
