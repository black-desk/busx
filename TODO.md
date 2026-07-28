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

## Testing

### tuiprobe README API table is incomplete

The API table in `crates/tuiprobe/README.md` lists `screen_contents()` /
`contains()` but omits several public methods that exist and are used by the
busx test suite: `wait_for_rect`, `wait_for_rect_with_timeout`,
`screen_contents_crop`, and the `TuiProbeBuilder` (`probe.builder().cols()…`). A
test author reading only the README would miss them.

- Suspected location: `crates/tuiprobe/README.md` (the "Output" / builder
  tables), vs. the actual surface in `crates/tuiprobe/src/harness.rs`.
