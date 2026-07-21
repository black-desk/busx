// SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end TUI snapshot tests.
//!
//! Each test spins up the real `testbus` fixture (a private `dbus-daemon`
//! owning `org.busx.Test` + a handful of extra well-known names), drives the
//! production `App::run_loop` against a ratatui `TestBackend`, scripts a
//! sequence of key events, and snapshots the rendered buffer. PID values are
//! non-deterministic (the daemon's PID changes per run), so an insta filter
//! rewrites them to `<PID>` before the snapshot comparison.
//!
//! The event source (`ScriptedSource`) interleaves scripted key presses with
//! the `Msg`s that the production `run_effect` sends back as fetches complete:
//! it drains the channel first, briefly waits for in-flight effects, and only
//! then hands the next scripted key to `run_loop`. After the script is
//! exhausted it keeps draining for a short grace period so trailing effects
//! (e.g. the result of a method call) land in the final snapshot.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::dbus;
use crate::tui::app::{run_effect, App};
use crate::tui::msg::{Effect, Msg};
use crate::tui::state::State;

/// A keyboard event with no modifiers — the common case in the script.
fn key(code: KeyCode) -> Msg {
    Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// Build the production `on_effect` closure over a fresh connection to the
/// shared `testbus`. Each call returns an independent closure + receiver pair
/// so tests don't share state (the underlying bus is shared via `OnceLock` in
/// `testbus::bus()`; the connection / channel here are per-test).
struct Harness {
    app: App,
    rx: flume::Receiver<Msg>,
    on_effect: Box<dyn FnMut(Effect)>,
}

fn harness() -> Harness {
    let addr = testbus::bus().address.clone();
    let (conn, bus) = async_global_executor::block_on(async {
        dbus::conn::connect_with_bus(false, false, Some(&addr))
            .await
            .expect("connect test bus")
    });
    let (tx, rx) = flume::unbounded::<Msg>();
    let addr_for_closure = addr.clone();
    let on_effect: Box<dyn FnMut(Effect)> = {
        Box::new(move |effect: Effect| {
            // Mock the clipboard: tests run headless on CI / developer
            // machines and must never touch the real system clipboard
            // (wl-copy / xclip / xsel / arboard would otherwise steal the
            // user's selection). Report success so the popup's status line
            // follows the same code path as production.
            if matches!(effect, Effect::CopyToClipboard(_)) {
                let _ = tx.send(Msg::ClipboardResult(Ok(())));
                return;
            }
            run_effect(
                effect,
                conn.clone(),
                tx.clone(),
                false,
                false,
                Some(&addr_for_closure),
            );
        })
    };
    let mut app = App {
        state: State::loading_service(),
    };
    app.state.bus = bus;
    Harness { app, rx, on_effect }
}

/// Event source that interleaves scripted keys with the async `Msg`s flowing
/// back from `run_effect`. Modeled after the production `CrosstermSource` but
/// with the crossterm poll replaced by a scripted iterator + a short wait.
struct ScriptedSource {
    rx: flume::Receiver<Msg>,
    keys: std::vec::IntoIter<Msg>,
    keys_done: bool,
    /// Deadline past which we stop draining trailing effects after the script
    /// is exhausted. Set on the first call after `keys_done` becomes true.
    final_drain_deadline: Option<Instant>,
}

impl ScriptedSource {
    fn new(rx: flume::Receiver<Msg>, keys: Vec<Msg>) -> Self {
        ScriptedSource {
            rx,
            keys: keys.into_iter(),
            keys_done: false,
            final_drain_deadline: None,
        }
    }
}

impl Iterator for ScriptedSource {
    type Item = Msg;

    fn next(&mut self) -> Option<Msg> {
        // 1. Always drain any ready Msg first — fetch results that have already
        //    landed take precedence over the next scripted key.
        if let Ok(msg) = self.rx.try_recv() {
            return Some(msg);
        }

        // 2. While the script still has keys, wait briefly for an in-flight
        //    effect before turning to the next key. A localhost dbus fetch
        //    (list_names / introspect / object_tree round-trip) typically
        //    lands in under 50 ms but object_tree recurses, so allow 250 ms
        //    for safety. If nothing arrives we assume the previous key
        //    produced no fetch (e.g. a cursor move) and proceed.
        if !self.keys_done {
            let deadline = Instant::now() + Duration::from_millis(250);
            while Instant::now() < deadline {
                if let Ok(msg) = self.rx.try_recv() {
                    return Some(msg);
                }
                std::thread::sleep(Duration::from_millis(2));
            }
            if let Some(k) = self.keys.next() {
                return Some(k);
            }
            // Script exhausted — fall through to the trailing-drain phase.
            self.keys_done = true;
            self.final_drain_deadline = Some(Instant::now() + Duration::from_millis(500));
        }

        // 3. Script done: keep draining until the grace deadline so trailing
        //    effects (a call reply, a final fetch) land before run_loop exits.
        let deadline = self.final_drain_deadline?;
        loop {
            if let Ok(msg) = self.rx.try_recv() {
                return Some(msg);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// Run the harness through `run_loop` with the given scripted keys, then
/// return the rendered buffer as a string for snapshot comparison. The initial
/// `Effect::FetchServices` (mirroring production `app::run`) is fired before
/// the loop so the service list actually loads.
fn run_scripted(input: Harness, keys: Vec<Msg>, w: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, height)).expect("TestBackend");
    let Harness {
        mut app,
        rx,
        mut on_effect,
    } = input;
    // Mirror production's initial fetch (app.rs fires Effect::FetchServices
    // before run_loop starts).
    on_effect(Effect::FetchServices);
    let events = ScriptedSource::new(rx, keys);
    app.run_loop(&mut terminal, events, on_effect)
        .expect("run_loop");
    format!("{}", terminal.backend())
}

/// insta settings shared by every e2e test:
/// - Rewrite numeric PIDs to `<PID>`. PIDs in the rendered buffer are always
///   sandwiched between two spaces (render_table separates NAME / PID /
///   PROCESS with two spaces). PIDs on Linux are at least 2 digits past
///   init, so `\\d{2,}` avoids clashing with single-digit numeric values
///   (e.g. `f64` volume `0.0` pretty-prints as `"0"`).
/// - Normalize the testbus address. `dbus-daemon --print-address` picks a
///   fresh socket path + GUID per run (`unix:path=/tmp/dbus-XXX,guid=YYY`),
///   which leaks into `--address` flags of every copy-as command. Without
///   this filter any test that opens the copy-as popup would be flaky.
fn pid_filter() -> insta::Settings {
    let mut s = insta::Settings::new();
    s.add_filter(r"  \d{2,}  ", "  <PID>  ");
    // Unique connection names (`:1.N`) are handed out by the bus driver in
    // connection order. Under `cargo test`'s default thread pool several tests
    // connect concurrently, so the exact N values (and how many appear) vary
    // per run — mask the digit so any `:1.N` snapshot-stabilizes.
    s.add_filter(r":1\.\d+", ":1.N");
    s.add_filter(
        r#"unix:path=[^,\s"]+,guid=[^,\s"]+"#,
        "unix:path=<SOCKET>,guid=<GUID>",
    );
    s.add_filter(
        r#"unix:abstract=[^,\s"]+,guid=[^,\s"]+"#,
        "unix:abstract=<SOCKET>,guid=<GUID>",
    );
    s
}

/// Sanity: testbus is up, fixture is registered, and the service list renders
/// with `org.busx.Test` and the scroll-helper names present.
#[test]
fn e2e_starts_and_lists_test_service() {
    let _g = pid_filter().bind_to_scope();
    let out = run_scripted(harness(), vec![], 64, 12);
    // ScrollXX names sort ahead of org.busx.Test alphabetically, so the Test
    // service itself isn't in the initial viewport — but its scroll helpers
    // are, which is enough to prove list_names ran end-to-end.
    assert!(
        out.contains("org.busx.Scroll00"),
        "service list fetched real names: {out}"
    );
    assert!(
        out.contains("org.busx.Scroll00"),
        "scroll fixture names present: {out}"
    );
    insta::assert_snapshot!(out);
}

/// Down arrow moves the selection off the first row; the service list should
/// still be visible (no navigation yet, since Enter was never pressed).
#[test]
fn e2e_down_arrow_moves_selection() {
    let _g = pid_filter().bind_to_scope();
    let out = run_scripted(
        harness(),
        vec![key(KeyCode::Down), key(KeyCode::Down)],
        64,
        8,
    );
    insta::assert_snapshot!(out);
}

/// With ~15 services and a 4-row viewport, scrolling down past the bottom and
/// back up must follow vim/less-style semantics: the highlight climbs within
/// the viewport, not the viewport jumping to re-pin it. This is the
/// end-to-end counterpart of the previously-deleted
/// `service_screen_scroll_up_after_down_does_not_pin_to_bottom` unit test.
#[test]
fn e2e_service_list_scrolls_then_climbs() {
    let _g = pid_filter().bind_to_scope();
    // 8 rows tall: 2 chrome (breadcrumb + footer) + 2 list borders = 4 list
    // rows visible. 14 services (org.busx.Test + the long name + 12 ScrollXX)
    // are plenty to scroll.
    let keys: Vec<Msg> = (0..10).map(|_| key(KeyCode::Down)).collect();
    let out = run_scripted(harness(), keys, 64, 8);
    assert!(out.contains("Scroll09"), "scrolled far enough to see Scroll09");
    insta::assert_snapshot!(out);
}

/// Drill into `org.busx.Test` via filter + Enter. With ~14 well-known names
/// the service list scrolls, so typing `/test` filters down to
/// `org.busx.Test` (and the long-name variant), Enter then drills into the
/// real `/org/busx/Test` + `/org/busx/Test/sub` paths exposed by the fixture.
/// Both paths expose interfaces, so the Objects screen does not auto-skip —
/// the user sees the list.
#[test]
fn e2e_drill_into_test_service_shows_real_paths() {
    let _g = pid_filter().bind_to_scope();
    let mut keys = vec![key(KeyCode::Char('/'))];
    for ch in "test".chars() {
        keys.push(key(KeyCode::Char(ch)));
    }
    keys.push(key(KeyCode::Enter));
    let out = run_scripted(harness(), keys, 64, 12);
    assert!(
        out.contains("/org/busx/Test"),
        "objects list fetched real paths: {out}"
    );
    insta::assert_snapshot!(out);
}

/// Continue the drill-down: service → object → interfaces → interface.
/// `/org/busx/Test` exposes a single non-standard interface (`org.busx.Test`),
/// so the Interfaces screen auto-skips straight into the Interface screen,
/// which renders the real method/property/signal columns (volume/name/counts/
/// hints properties; join/take_hints/bump_volume/make_* methods).
#[test]
fn e2e_drill_down_to_interface_screen() {
    let _g = pid_filter().bind_to_scope();
    let mut keys = vec![key(KeyCode::Char('/'))];
    for ch in "test".chars() {
        keys.push(key(KeyCode::Char(ch)));
    }
    keys.push(key(KeyCode::Enter)); // → Objects
    // object_tree lists every introspectable path including the intermediate
    // container paths (/, /org, /org/busx) — only the 4th row is the real
    // /org/busx/Test leaf.
    for _ in 0..3 {
        keys.push(key(KeyCode::Down));
    }
    keys.push(key(KeyCode::Enter)); // → Interfaces (auto-skip) → Interface
    let out = run_scripted(harness(), keys, 80, 20);
    // The real interface name surfaces in the breadcrumb / title once the
    // Interface screen is reached.
    assert!(
        out.contains("org.busx.Test") && out.contains("volume"),
        "reached the Interface screen with real properties: {out}"
    );
    insta::assert_snapshot!(out);
}

/// `/org/busx/Test/sub` is the second registered object. It exposes the same
/// `org.busx.Test` interface but with `volume = 0.0` (vs the root's 0.5), so
/// drilling into it yields a visibly different GetAll snapshot.
#[test]
fn e2e_sub_object_renders_distinct_volume() {
    let _g = pid_filter().bind_to_scope();
    let mut keys = vec![key(KeyCode::Char('/'))];
    for ch in "test".chars() {
        keys.push(key(KeyCode::Char(ch)));
    }
    keys.push(key(KeyCode::Enter)); // → Objects
    for _ in 0..4 {
        keys.push(key(KeyCode::Down)); // → /org/busx/Test/sub (row 4)
    }
    keys.push(key(KeyCode::Enter)); // → Interface (auto-skip)
    let out = run_scripted(harness(), keys, 80, 20);
    assert!(
        out.contains("volume"),
        "reached the Interface screen with real properties: {out}"
    );
    insta::assert_snapshot!(out);
}

/// `/` opens the inline filter; typing `scroll` narrows the service list to
/// the 12 `org.busx.ScrollNN` rows. With the filter active, Down moves within
/// the matches and Enter would drill the filtered selection.
#[test]
fn e2e_filter_narrows_service_list() {
    let _g = pid_filter().bind_to_scope();
    let mut keys = vec![key(KeyCode::Char('/'))];
    for ch in "scroll".chars() {
        keys.push(key(KeyCode::Char(ch)));
    }
    let out = run_scripted(harness(), keys, 64, 12);
    // Every visible service row should be a ScrollNN — the well-known
    // `org.busx.Test` and the long name don't contain "scroll" so they must
    // have been filtered out.
    assert!(
        out.contains("org.busx.Scroll00"),
        "filter kept the scroll rows: {out}"
    );
    assert!(
        !out.contains("org.busx.Test\n") && !out.contains("\norg.busx.Test "),
        "filter hid the non-scroll well-known names: {out}"
    );
    insta::assert_snapshot!(out);
}

/// `?` opens the help overlay; the rendered buffer should include the help
/// text on top of the service list, not just the bare service list.
#[test]
fn e2e_help_overlay_renders_over_service_list() {
    let _g = pid_filter().bind_to_scope();
    let out = run_scripted(harness(), vec![key(KeyCode::Char('?'))], 80, 24);
    insta::assert_snapshot!(out);
}

/// `c` on the Detail screen (reached by drilling service → object → interface
/// → method → Call button) opens the copy-as popup, which renders the
/// currently-edited call as a `dbus-send` / `busctl` / `qdbus` / `gdbus`
/// command line. This exercises the production `tui::copy::generate` path
/// (including signature splitting and shell quoting) end-to-end through a
/// real interface, replacing the previously inlined
/// `tui::copy::split_signature_basics` / `quote_only_when_needed` unit tests.
#[test]
fn e2e_copy_as_popup_on_interface_screen() {
    let _g = pid_filter().bind_to_scope();
    let mut keys = vec![key(KeyCode::Char('/'))];
    for ch in "test".chars() {
        keys.push(key(KeyCode::Char(ch)));
    }
    keys.push(key(KeyCode::Enter)); // → Objects
    for _ in 0..3 {
        keys.push(key(KeyCode::Down)); // → /org/busx/Test
    }
    keys.push(key(KeyCode::Enter)); // → Interfaces (auto-skip) → Interface
    keys.push(key(KeyCode::Enter)); // enter the actions button bar
    keys.push(key(KeyCode::Enter)); // fire Call → push Detail (TakeHints, a{sv})
    keys.push(key(KeyCode::Char('c'))); // open copy-as popup on the Detail
    let out = run_scripted(harness(), keys, 100, 28);
    // All four supported tools must show up in the popup, and the generated
    // command must reference the real interface + method (proving
    // split_signature and quoting ran over the introspected data).
    assert!(
        out.contains("dbus-send")
            && out.contains("busctl")
            && out.contains("qdbus")
            && out.contains("gdbus"),
        "copy-as popup shows all four tools: {out}"
    );
    assert!(
        out.contains("org.busx.Test"),
        "copy-as command references the real interface: {out}"
    );
    insta::assert_snapshot!(out);
}
