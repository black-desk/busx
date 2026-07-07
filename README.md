<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# busx

[![checks][badge-shields-io-checks]][actions]
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

en | [zh_CN](README.zh_CN.md)

> [!NOTE]
>
> This English README is translated from the Chinese version and may contain
> errors.

`busx` is a D-Bus command-line tool written in Rust (on top of [zbus]), aiming
to replace `dbus-send` / `busctl` / `qdbus` and fix each of their pain points at
once:

- Input follows `busctl` style (signature string + positional args) and **fully
  supports nesting and empty containers** — closing `dbus-send`'s hard gap;
- Output is **human-friendly text by default**; `--json` switches to
  **type-tagged JSON** (every value is `{"type":..,"data":..}`, and monitoring
  is NDJSON — one object per line), script-friendly and pipeable to `jq` / python;
- **It does not repeat sd-bus's mistake**: dicts with non-string keys (e.g.
  `a{uu}`) render as `[{"key":..,"value":..}]` instead of crashing (cf.
  systemd#32904);
- Ships **dynamic shell completion** (bash/zsh) that introspects the bus live;
- **Single binary, zero runtime dependencies** (pure Rust, no libdbus).

[zbus]: https://crates.io/crates/zbus

## Features

- `list` — list service names + PID + process (human table, or `--json` array of objects).
- `tree SVC` — draw the object-path tree of a single service.
- `introspect` — list an object's interfaces / methods / signals / properties.
- `call SVC OBJ IFACE METHOD SIG ARGS...` — call a method (SIG is a distinct, completable required arg; busctl-style input, arbitrary nesting supported).
- `get` / `set` — read (no property names = `GetAll`) / write properties.
- `monitor` — monitor bus messages, filter by match rule (`--json` emits NDJSON with `PropertiesChanged` decoded).
- `completion` — generate a dynamic shell-completion script (live-completes services/paths/interfaces/methods/signature/properties).

## Build & install

You need a Rust toolchain and a D-Bus environment (Linux only). The system must
provide `dbus-daemon` (used at run/test time).

```bash
cargo build --release        # binary at target/release/busx
cargo install --path .       # or install straight to ~/.cargo/bin
```

## Usage

```bash
# List services (defaults to the session bus, falling back to the system bus)
busx list

# Introspect an object
busx introspect org.freedesktop.systemd1 /org/freedesktop/systemd1

# Call a method (SIG is a distinct required arg; ListUnits takes no args, so SIG is "")
busx call org.freedesktop.systemd1 /org/freedesktop/systemd1 \
  org.freedesktop.systemd1.Manager ListUnits ""

# Nested input (a{sv} containing an array — impossible with dbus-send; 'a{sv}' is SIG):
busx call org.example /obj org.example.Iface Method \
  'a{sv}' 1 'hint' 'a' 's' 2 'a' 'b'

# Read properties (no property names = GetAll)
busx get org.freedesktop.systemd1 /org/freedesktop/systemd1 \
  org.freedesktop.systemd1.Manager

# Monitor signals; --json emits NDJSON to pipe to external jq
busx --json monitor --signals --interface org.freedesktop.DBus.Properties \
  --member PropertiesChanged | jq 'select(.args[1] != {})'

# Generate bash completion (once sourced, it live-introspects the bus to
# complete services/paths/interfaces/methods)
busx completion bash > /etc/bash_completion.d/busx
```

Output is human-friendly text by default; `--json` switches to type-tagged JSON
(`monitor` is NDJSON) — pipe to an external `jq` / python for pretty-printing or
field transformation. All diagnostics (errors, warnings) go to stderr with the
`busx:` prefix; exit code is `0` on success, `1` on failure. Piping into
`less`/`head` does not panic (SIGPIPE is handled the Unix way).

See the [design doc] for details.

[design doc]: docs/superpowers/specs/2026-07-07-busx-design.md

## Roadmap

1. **TUI**: built on this crate's modules (a second `[[bin]]`, or extract a lib
   then), providing interactive browsing / call / monitor; with
   `copy as dbus-send / busctl / gdbus`.
2. `emit` (emit signals), pcapng `capture`.
3. `--host` / `--machine` remote and container buses.
4. A bytestring string view for `ay` and other value-rendering enhancements.
5. Re-evaluate an embedded `busx jq` subcommand if `jaq` ever publishes a
   reusable flag-parsing library entry.

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
