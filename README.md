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
  introspect  Show interfaces/methods/signals/properties of an object
  call        Call a method
  get         Get properties (no property names => GetAll)
  set         Set a property
  monitor     Monitor bus messages
  help        Print this message or the help of the given subcommand(s)

Options:
      --user                      Connect to the session bus (the default falls back to the system bus on failure)
      --system                    Connect to the system bus
      --address <ADDRESS>         Connect to the bus at ADDRESS (e.g. unix:path=...)
  -v...                           Increase log verbosity (-v / -vv / -vvv)
      --log <PATH>                TUI log file (default: $XDG_CACHE_HOME/busx/busx.log)
      --show-standard-interfaces  Show standard D-Bus interfaces in the TUI (hidden by default)
      --json                      Emit type-tagged JSON (default: human text)
  -h, --help                      Print help
  -V, --version                   Print version
```

A few common examples:

```bash
# Interactive TUI (no subcommand)
busx

# List services on the bus (defaults to the session bus, falling back to system)
busx list

# Introspect an object
busx introspect org.freedesktop.systemd1 /org/freedesktop/systemd1

# Call a method (SIG is a distinct required arg; ListUnits takes none, so SIG is "")
busx call org.freedesktop.systemd1 /org/freedesktop/systemd1 \
  org.freedesktop.systemd1.Manager ListUnits ""

# Read properties (no property names = GetAll)
busx get org.freedesktop.systemd1 /org/freedesktop/systemd1 \
  org.freedesktop.systemd1.Manager

# Monitor signals; --json emits NDJSON, easy to pipe to jq
busx --json monitor --signals --interface org.freedesktop.DBus.Properties \
  --member PropertiesChanged | jq 'select(.args[1] != {})'

# Enable completion: add this to ~/.bashrc (or ~/.zshrc for zsh) and restart
# your shell — it live-introspects the bus to complete services/paths/etc.
eval "$(busx completion bash)"
```

By default the output is human-friendly text; `--json` switches to **type-tagged
JSON** (`monitor` is NDJSON) — every value is `{"type":..,"data":..}`,
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
- **copy-as**: any operation (method call, get / set property, listen for
  signals) can be copied as a command line in `dbus-send` / `busctl` / `qdbus` /
  `gdbus` form

## Roadmap

Planned refactors and known issues are tracked in [TODO.md](TODO.md).

The `tree` subcommand was removed; its capability (locating objects under a
service) will return as an in-TUI search.

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
