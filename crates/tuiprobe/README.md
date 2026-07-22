<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
SPDX-License-Identifier: MIT
-->

# tuiprobe

PTY-based integration testing for TUI applications.

Spin up a TUI app (ratatui, cursive, raw crossterm, …) as a **real child
process** inside a pseudo-terminal, send keyboard / mouse events, wait for the
rendered output to reach a known state, and snapshot it.

The terminal emulator is [`alacritty_terminal::Term`] — the same engine that
powers the [Alacritty] terminal emulator. All ANSI escape sequences (cursor
movement, SGR colors, erase operations, alternate screen, …) are handled with
production-grade correctness.

[Alacritty]: https://alacritty.org

## Quick start

```rust
use portable_pty::CommandBuilder;
use tuiprobe::{KeyCode, TuiProbe};

let mut probe = TuiProbe::new(80, 24)?;

let mut cmd = CommandBuilder::new("my-tui-app");
cmd.arg("--config");
cmd.arg("test.toml");
probe.spawn(cmd)?;

// Wait for the app to render its initial screen.
probe.wait_for(|s| s.contains("Welcome"))?;

// Navigate.
probe.send_key(KeyCode::Down);
probe.send_key(KeyCode::Enter);
probe.wait_for(|s| s.contains("Settings"))?;

// Snapshot the rendered terminal.
insta::assert_snapshot!(probe.screen_contents());

probe.send_key(KeyCode::Char('q'));
# Ok::<(), tuiprobe::Error>(())
```

## Why?

Testing a TUI app with a mock backend (e.g. ratatui's `TestBackend`) skips
everything between `main()` and the first render — CLI parsing, terminal setup,
the real crossterm event loop, raw-mode negotiation. Bugs in any of those go
uncaught.

`tuiprobe` runs the **real binary** in a **real PTY**, so you test the exact
code path your users hit. The trade-off is that output comes as ANSI escape
sequences rather than a ready-to-read buffer; `tuiprobe` bridges that gap with a
full terminal emulator so you get clean text back.

## API overview

### Input

| Method                                    | Description                                  |
| ----------------------------------------- | -------------------------------------------- |
| `send_key(KeyCode)`                       | Single key press (Enter, Down, Char('a'), …) |
| `send_key_with_mods(key, KeyModifiers)`   | Ctrl/Alt/Shift combos                        |
| `send_text("&str")`                       | Type a string (one Char event per character) |
| `mouse_click(col, row, MouseButton)`      | Click at screen coordinates                  |
| `mouse_scroll(col, row, ScrollDirection)` | Scroll wheel                                 |

### Waiting (Cypress-style)

| Method                                      | Description                                   |
| ------------------------------------------- | --------------------------------------------- |
| `wait_for(\|screen\| screen.contains("x"))` | Custom condition, polls until true or timeout |
| `wait_for_with_timeout(cond, 2s)`           | Same, with a custom timeout                   |

### Output

| Method              | Description                                                   |
| ------------------- | ------------------------------------------------------------- |
| `screen_contents()` | Full visible screen as a trimmed string                       |
| `contains("text")`  | Quick check for text presence                                 |
| `cell(row, col)`    | Access the `alacritty_terminal::Cell` (char + colors + flags) |

### Process control

| Method               | Description                            |
| -------------------- | -------------------------------------- |
| `is_running()`       | Check if the child is still alive      |
| `wait_exit()`        | Block until child exits, return status |
| `resize(cols, rows)` | Resize the PTY window                  |

## How it works

```
 ┌───────────────────┐           ┌──────────────────────┐
 │   TuiProbe (you)  │           │   Child process      │
 │                   │           │  (your TUI app)      │
 │  send_key(Enter)  │  bytes    │                      │
 │  ────────────────►│──────────►│  crossterm::read()   │
 │                   │  (\r)     │  ratatui::draw()     │
 │  screen_contents()│  ANSI     │                      │
 │  ◄────────────────│◄──────────│  stdout (diff render)│
 │                   │  escapes  │                      │
 │  ┌─────────────┐  │           └──────────────────────┘
 │  │alacritty_   │  │
 │  │terminal::Term│ │
 │  │  (grid)     │  │
 │  └─────────────┘  │
 └───────────────────┘
```

1. **PTY** (`portable-pty`): creates a pseudo-terminal pair (master + slave).
   The child gets the slave as its stdin/stdout/stderr; `tuiprobe` holds the
   master.

2. **Background reader thread**: clones the PTY reader **once** at spawn time
   and continuously drains it into an mpsc channel. The main thread reads from
   the channel — never touching the PTY fd directly. (This avoids the data-loss
   bug that arises from cloning the reader on every `read()` call.)

3. **Terminal emulator** (`alacritty_terminal::Term`): PTY output bytes are fed
   through `vte`'s parser into `Term`, which maintains the screen grid — exactly
   what Alacritty does to render its window.

## Key encoding details

`tuiprobe` encodes keys the way **crossterm in raw mode** expects them. This
matters because most Rust TUI apps (ratatui, cursive) use crossterm as their
backend.

The critical gotcha: **Enter is `\r` (CR, 0x0D), not `\n` (LF, 0x0A)**. In raw
mode crossterm maps `\r` → `KeyCode::Enter` but leaves `\n` as `Ctrl+J`. If your
key encoder sends `\n` for Enter (as some libraries do), Enter will silently not
work and you'll spend hours debugging.

## License

MIT
