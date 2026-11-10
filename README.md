<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# busx

[![checks][badge-shields-io-checks]][actions]
[![codecov][badge-shields-io-codecov]][codecov]
[![commit activity][badge-shields-io-commit-activity]][commits]
[![contributors][badge-shields-io-contributors]][contributors]
[![release date][badge-shields-io-release-date]][releases]
![commits since release][badge-shields-io-commits-since-release]

[badge-shields-io-checks]:
  https://img.shields.io/github/check-runs/black-desk/busx/master
[actions]: https://github.com/black-desk/busx/actions
[badge-shields-io-commit-activity]:
  https://img.shields.io/github/commit-activity/w/black-desk/busx/master
[commits]: https://github.com/black-desk/busx/commits/master
[badge-shields-io-contributors]:
  https://img.shields.io/github/contributors/black-desk/busx
[contributors]: https://github.com/black-desk/busx/graphs/contributors
[badge-shields-io-release-date]:
  https://img.shields.io/github/release-date/black-desk/busx
[releases]: https://github.com/black-desk/busx/releases
[badge-shields-io-commits-since-release]:
  https://img.shields.io/github/commits-since/black-desk/busx/latest
[badge-shields-io-codecov]:
  https://codecov.io/github/black-desk/busx/graph/badge.svg?token=9DDJPR1SVE
[codecov]: https://codecov.io/github/black-desk/busx

en | [zh_CN](README.zh_CN.md)

> [!WARNING]
>
> This project is in an **early development stage**; its code has **never been
> reviewed by a human**. Do not use it in production.

> [!NOTE]
>
> This English README is translated from the Chinese version and may contain
> errors.

`busx` is a D-Bus TUI / command-line tool written in Rust (on top of [zbus]), in
the lineage of `dbus-send` and d-feet / d-spy.

[zbus]: https://crates.io/crates/zbus

## Features

- Run `busx` with no subcommand to open the interactive browser (service →
  objects → interfaces → methods / properties / signals).
- Run a subcommand (`list` / `call` / `get` / …) for plain command-line use —
  easy to drop into scripts.

```bash
busx --help
```

```text
D-Bus CLI (dbus-send/busctl/qdbus replacement)

Usage: busx [OPTIONS] [COMMAND]

Commands:
  list        List service names on the bus
  introspect  Dump the raw Introspect() XML for an object, verbatim
  call        Call a method
  get         Get properties (no property names => GetAll)
  set         Set a property
  monitor     Monitor bus messages
  tree        Recursively list an object-path tree of a service
  emit        Emit a D-Bus signal (for testing listeners on a private bus)
  help        Print this message or the help of the given subcommand(s)

Options:
      --user                      Connect to the session bus (the default falls back to the system bus on failure)
      --system                    Connect to the system bus
      --address <ADDRESS>         Connect to the bus at ADDRESS (e.g. unix:path=...)
  -v...                           Increase log verbosity (-v / -vv / -vvv)
      --log <PATH>                TUI log file (default: $XDG_CACHE_HOME/busx/busx.log)
      --show-standard-interfaces  Show standard D-Bus interfaces hidden by default
      --json                      Emit JSON (default: human text)
  -h, --help                      Print help (see more with '--help')
  -V, --version                   Print version
```

A few common examples:

```bash
# Interactive TUI (no subcommand)
busx

# List services on the bus (defaults to the session bus, falling back to system)
busx list

# Introspect an object
busx introspect org.freedesktop.systemd1 /org/freedesktop/systemd1   # raw Introspect() XML

# Call a method (SIG is a distinct required arg; ListUnits takes none, so SIG is "")
busx call org.freedesktop.systemd1 /org/freedesktop/systemd1 \
  org.freedesktop.systemd1.Manager ListUnits ""

# Read properties (no property names = GetAll)
busx get org.freedesktop.systemd1 /org/freedesktop/systemd1 \
  org.freedesktop.systemd1.Manager

# Monitor signals; --json emits NDJSON, easy to pipe to jq. Once it is live
# on the bus busx prints a {"event":"ready"} line first — scripts can wait
# for it (no sleep race) and then generate traffic; jq filters it out here.
busx --json monitor --type signal --interface org.freedesktop.DBus.Properties \
  --member PropertiesChanged | jq 'select(.type)'

# Enable completion: add this to ~/.bashrc (or ~/.zshrc for zsh) and restart
# your shell — it live-introspects the bus to complete services/paths/etc.
eval "$(busx completion bash)"
```

By default the output is human-friendly text; `--json` switches to **type-tagged
JSON** (`monitor` is NDJSON) — every D-Bus value is `{"type":..,"data":..}`,
preserving full D-Bus type information for piping to external `jq` / python. All
diagnostics (errors, warnings) go to stderr with the `busx:` prefix; exit code
is `0` on success, `1` on failure; piping into `less`/`head` does not panic
(SIGPIPE is handled the Unix way).

### CLI mode: vs `busctl`

- **Output**: human-friendly text by default; `--json` emits type-tagged JSON
  that preserves full D-Bus type information for scripts / `jq`.
- **Default bus**: connects to the session bus by default.
- **Pure-Rust single binary**: no libdbus, and no systemd dependency.

### TUI mode: vs d-feet

- **Runs in a terminal**: no graphical environment needed — works over SSH, in a
  TTY, in a container; d-feet / d-spy are GTK apps that need a desktop.
- **Service browser**: a shared object/interface tree and member categories
  (properties, methods, signals). At 80 columns and above, navigation stays
  beside the inspector; narrower terminals show the focused pane. `Tab` /
  `Shift+Tab` move focus, `h/l` switch member categories or fold objects, and
  `j/k` select rows (arrow keys remain available). `gg/G` jump to the first/last
  item; `n` returns to navigation. Navigation distinguishes object paths
  (`▸ /App47`) from indented interfaces (`@ org.gnome.SessionManager.App`).
  Interface names retain their full namespace; names that exceed the pane width
  are truncated with an ellipsis, and the selected full name is shown above.
  Leaf objects start folded; unbranched container paths are compressed.
  Breadcrumbs use `›` to keep object-path slashes unambiguous.
- **Inline operations**: select a member to inspect its signature, property
  permissions (`r` / `w` / `rw`), and current value. Fill parameters and
  activate `Call` / `Write` explicitly; parameters and results stay together.
  Parameter inputs share an aligned label column. Focusing an input starts in
  normal mode: `j/k` selects a field, `i` enters insert mode, and `Esc` returns
  to normal mode without moving focus. Moving down from the last field focuses
  the action row; up returns to the last field, and `h/l` selects an action. In
  insert mode, `Tab` moves between fields; `Enter` finishes editing and focuses
  the actions without executing. `Expand result` opens large replies
  full-screen, and `Esc` returns to the editor. Basic values are entered
  verbatim, including spaces and empty strings; containers use busctl-style
  count-prefixed tokens, e.g. `2 hello world` for `as`. Ordinary characters,
  including `q`, type normally inside inputs; `Ctrl+C` closes the current tab
  from any input.
- **Independent tabs**: `t` opens a new service browser; `gt/gT` switch to the
  next/previous tab, and `{number}gt` jumps to a tab. Each tab keeps its own
  service, object, interface, inputs, results, filter, and scroll position. `q`
  closes the current tab; closing the last tab exits. `Ctrl+C` closes the
  current tab even while editing. These letter commands apply in normal mode;
  typed characters are preserved while editing. The top `+` also opens a tab,
  tab titles switch tabs, and each tab's `×` closes it.
- **Contextual footer**: one line shows actions for the focused area and the
  help entry. Status messages temporarily replace the action hints; global tab
  shortcuts are listed in help. While editing, leave insert mode with `Esc`
  before pressing `?` for help.
- **Watch in place**: `Watch` changes the current tab into a single signal,
  property-change, or method-call subscription. Use `t` to browse elsewhere
  while this tab continues listening. `p` freezes the display while collection
  continues, `s` stops but keeps the messages, `x` clears retained events, and
  `End` follows the latest event. `Esc` stops the subscription and restores that
  tab's browser, inputs, and selection; closing the tab also stops it. Each
  watch retains at most 1,000 events; method-call watching requires permission
  to become a bus monitor.
- **copy-as**: `c` previews the current operation as a `dbus-send` / `busctl` /
  `qdbus` / `gdbus` / `busx` command; choose a tool and press `Enter` to copy.
  `y` copies result or watch text. Unsupported tool formats are marked in the
  preview.
- **Inline filter**: `/` filters services, object/interface navigation, or the
  active member tab by substring (case-insensitive). Type to narrow, `Up/Down`
  to select, `Enter` to open, and `Esc` to clear. `?` shows contextual keyboard
  help outside inputs.

## Install

```bash
# Install the latest from GitHub (binary lands in ~/.cargo/bin, on $PATH if you
# use rustup)
cargo install --git https://github.com/black-desk/busx

# Update later (one-time setup: `cargo install cargo-update`). -g is required:
# git-originating packages are skipped by default.
cargo install-update -g busx       # or `cargo install-update -ag` for everything
```

## License

Unless otherwise specified, the code of this project is open source under the
GNU General Public License version 3 or any later version, while documentation,
configuration files, and scripts used in development and maintenance are open
source under the MIT License.

This project complies with the [REUSE specification]. You can use
[reuse-tool](https://github.com/fsfe/reuse-tool) to generate the SPDX list for
this project:

```bash
reuse spdx
```

[REUSE specification]: https://reuse.software/spec-3.3/
