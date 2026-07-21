// SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end TUI snapshot tests.
//!
//! Each test spins up its own dedicated `testbus` fixture (a private
//! `dbus-daemon` owning `org.busx.Test` + a handful of extra well-known
//! names), drives the production `App::run_loop` against a ratatui
//! `TestBackend`, scripts a sequence of key events, and snapshots the
//! rendered buffer. PIDs and the cargo test binary's comm (`busx-<hash>`)
//! are environment-dependent, so an insta filter rewrites them to `<PID>`
//! / `<PROC>` before the snapshot comparison.
//!
//! The event source (`ScriptedSource`) interleaves scripted key presses with
//! the `Msg`s that the production `run_effect` sends back as fetches complete:
//! it drains the channel first, briefly waits for in-flight effects, and only
//! then hands the next scripted key to `run_loop`. After the script is
//! exhausted it keeps draining for a short grace period so trailing effects
//! (e.g. the result of a method call) land in the final snapshot.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use serial_test::serial;

use crate::dbus;
use crate::tui::app::{App, run_effect};
use crate::tui::msg::{Effect, Msg};
use crate::tui::state::State;

/// A keyboard event with no modifiers — the common case in the script.
fn key(code: KeyCode) -> Msg {
    Msg::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// RAII guard that prepend a tempdir of mock `wl-copy` / `xclip` / `xsel`
/// scripts to `PATH`. Each mock script appends its stdin to a shared log
/// file inside the tempdir, so tests that drive the copy-as popup can read
/// the log back to verify the command line busx actually handed to the
/// clipboard tool. Without this guard `arboard` (the fallback in
/// `app::write_to_clipboard`) would try to reach the real compositor and
/// fail / steal the user's selection on a developer machine.
///
/// `PATH` is a process-global environment variable, so the mock must be
/// serialized across e2e tests — hence the `#[serial]` attribute on every
/// test below. Each test still owns its own `TestBus` (per-test fixture
/// isolation), but the clipboard mock is the one shared resource.
struct ClipboardMock {
    log_path: std::path::PathBuf,
    /// Owns the tempdir holding the mock scripts + log file. Kept here so
    /// the dir lives exactly as long as the mock; reading `log_path` after
    /// `ClipboardMock::drop` would fail.
    _bin_dir: tempfile::TempDir,
    old_path: String,
}

impl ClipboardMock {
    fn new() -> Self {
        let bin_dir = tempfile::TempDir::new().expect("tempdir for clipboard mock");
        let log_path = bin_dir.path().join("clipboard.log");
        // All three tools busx tries get the same behavior: append stdin to
        // the log file and exit 0. wl-copy is tried first, so normally only
        // it runs; xclip/xsel exist as belt-and-suspenders in case a future
        // busx change reorders the tool list.
        let script = format!("#!/bin/sh\ncat >> {}\n", log_path.display());
        for name in ["wl-copy", "xclip", "xsel"] {
            let path = bin_dir.path().join(name);
            std::fs::write(&path, &script).expect("write mock script");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = std::fs::metadata(&path)
                    .expect("stat mock script")
                    .permissions();
                perms.set_mode(0o755);
                std::fs::set_permissions(&path, perms).expect("chmod mock script");
            }
        }

        let old_path = std::env::var("PATH").unwrap_or_default();
        // SAFETY: every e2e test is `#[serial]`, so no other test is
        // reading PATH concurrently. Other test binaries in the workspace
        // (integration tests under tests/) spawn `busx` via absolute path
        // (cargo-bin), so they don't observe this change.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("PATH", format!("{}:{}", bin_dir.path().display(), old_path));
        }

        ClipboardMock {
            log_path,
            _bin_dir: bin_dir,
            old_path,
        }
    }

    /// Read everything busx has copied via the mock tools so far. Blocks
    /// behind a short sleep because `write_to_clipboard` does **not** wait
    /// on the spawned tool (it can daemonize) — give the mock a beat to
    /// finish draining its stdin into the log.
    fn contents(&self) -> String {
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::read_to_string(&self.log_path).unwrap_or_default()
    }
}

impl Drop for ClipboardMock {
    fn drop(&mut self) {
        // SAFETY: same serialization argument as `set_var` in `new`.
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("PATH", &self.old_path);
        }
        // `bin_dir` (and thus `log_path`) is removed by TempDir's Drop.
    }
}

/// Build the production `on_effect` closure over a fresh connection to a
/// **dedicated** test bus (not the shared `testbus::bus()` singleton).
/// Each e2e test owns its `TestBus` for the duration of the test so the
/// daemon's `:1.x` table is fully deterministic (`:1.0` daemon, `:1.1`
/// fixture, `:1.2` this connection) regardless of how many other e2e tests
/// are running concurrently. Drop happens implicitly when `Harness` drops.
///
/// Returns the `ClipboardMock` separately so the test can hold it past
/// `run_scripted` (which consumes the `Harness`) and read the captured
/// clipboard log before the mock's tempdir is dropped.
struct Harness {
    app: App,
    rx: flume::Receiver<Msg>,
    on_effect: Box<dyn FnMut(Effect)>,
    #[allow(dead_code)]
    _bus: testbus::TestBus,
}

fn harness() -> (Harness, ClipboardMock) {
    let bus = testbus::bus_owned();
    let addr = bus.address.clone();
    let (conn, bus_kind) = async_global_executor::block_on(async {
        dbus::conn::connect_with_bus(false, false, Some(&addr))
            .await
            .expect("connect test bus")
    });
    let (tx, rx) = flume::unbounded::<Msg>();
    let addr_for_closure = addr.clone();
    // The production `run_effect` is used unmodified — including the
    // `Effect::CopyToClipboard` branch, which spawns `wl-copy` / `xclip` /
    // `xsel` via `write_to_clipboard`. The mock tools installed by
    // `ClipboardMock` catch that spawn, so the test never touches the real
    // clipboard and the copied text is captured for later assertion.
    let on_effect: Box<dyn FnMut(Effect)> = {
        Box::new(move |effect: Effect| {
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
    app.state.bus = bus_kind;
    (
        Harness {
            app,
            rx,
            on_effect,
            _bus: bus,
        },
        ClipboardMock::new(),
    )
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
        ..
    } = input;
    // Mirror production's initial fetch (app.rs fires Effect::FetchServices
    // before run_loop starts).
    on_effect(Effect::FetchServices);
    let events = ScriptedSource::new(rx, keys);
    app.run_loop(&mut terminal, events, on_effect)
        .expect("run_loop");
    format!("{}", terminal.backend())
}

/// insta settings shared by every e2e test.
///
/// The fixture process has a stable name (`testbus-fixture`, truncated to
/// `testbus-fixtur` by `TASK_COMM_LEN`) and the daemon's PID varies per
/// run, so only PID + the testbus socket/GUID need masking. PID column
/// is anchored via `^".{39}` (the prefix up to PID in a service-list
/// row) and replaced with a 7-char literal so subsequent columns stay
/// aligned.
fn pid_filter() -> insta::Settings {
    let mut s = insta::Settings::new();
    // PID column. Cols 0-39 (`"` + │ + 36 NAME + 2 sep) are captured as
    // group 1; the PID column itself is ` *\d{1,7}` (right-aligned digits,
    // exactly PID_W = 7 chars). Replacement is group 1 + `  <PID>`
    // (2 spaces + 5-char tag = 7 chars), so every column past PID stays
    // aligned.
    s.add_filter(r#"(?m)^(".{39}) *\d{1,7}"#, r#"${1}  <PID>"#);
    // PROCESS column holding the cargo test binary's comm
    // (`busx-<10 hex chars>` = 15 chars). Popups (help overlay, copy-as)
    // now clear the whole frame before rendering so the underlying
    // service list never leaks through, which means the hash only ever
    // appears in a plain service-list row's PROC column — never partially
    // obscured by a popup edge. Replacement is 15 chars so the trailing
    // `│` and any columns past PROC stay put. Other comms (`dbus-daemon`,
    // etc.) pass through unmasked.
    s.add_filter(r"busx-[0-9a-f]{10}", "<PROC>         ");
    // The testbus socket path + GUID — `dbus-daemon --print-address`
    // picks fresh values each run; they leak into `--address` flags.
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
#[serial]
fn e2e_starts_and_lists_test_service() {
    let _g = pid_filter().bind_to_scope();
    let out = run_scripted(harness().0, vec![], 64, 12);
    // ScrollA-L sort ahead of org.busx.Test alphabetically, so the Test
    // service itself isn't in the initial viewport — but its scroll helpers
    // are, which is enough to prove list_names ran end-to-end.
    assert!(
        out.contains("org.busx.ScrollA"),
        "service list fetched real names: {out}"
    );
    insta::assert_snapshot!(out);
}

/// Down arrow moves the selection off the first row; the service list should
/// still be visible (no navigation yet, since Enter was never pressed).
#[test]
#[serial]
fn e2e_down_arrow_moves_selection() {
    let _g = pid_filter().bind_to_scope();
    let out = run_scripted(
        harness().0,
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
#[serial]
fn e2e_service_list_scrolls_then_climbs() {
    let _g = pid_filter().bind_to_scope();
    // 8 rows tall: 2 chrome (breadcrumb + footer) + 2 list borders = 4 list
    // rows visible. 14 services (org.busx.Test + the long name + 12 ScrollX)
    // are plenty to scroll.
    let keys: Vec<Msg> = (0..10).map(|_| key(KeyCode::Down)).collect();
    let out = run_scripted(harness().0, keys, 64, 8);
    assert!(
        out.contains("ScrollJ"),
        "scrolled far enough to see ScrollJ: {out}"
    );
    insta::assert_snapshot!(out);
}

/// Drill into `org.busx.Test` via filter + Enter. With ~14 well-known names
/// the service list scrolls, so typing `/test` filters down to
/// `org.busx.Test` (and the long-name variant), Enter then drills into the
/// real `/org/busx/Test` + `/org/busx/Test/sub` paths exposed by the fixture.
/// Both paths expose interfaces, so the Objects screen does not auto-skip —
/// the user sees the list.
#[test]
#[serial]
fn e2e_drill_into_test_service_shows_real_paths() {
    let _g = pid_filter().bind_to_scope();
    let mut keys = vec![key(KeyCode::Char('/'))];
    for ch in "test".chars() {
        keys.push(key(KeyCode::Char(ch)));
    }
    keys.push(key(KeyCode::Enter));
    let out = run_scripted(harness().0, keys, 64, 12);
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
#[serial]
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
    let out = run_scripted(harness().0, keys, 80, 20);
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
#[serial]
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
    let out = run_scripted(harness().0, keys, 80, 20);
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
#[serial]
fn e2e_filter_narrows_service_list() {
    let _g = pid_filter().bind_to_scope();
    let mut keys = vec![key(KeyCode::Char('/'))];
    for ch in "scroll".chars() {
        keys.push(key(KeyCode::Char(ch)));
    }
    let out = run_scripted(harness().0, keys, 64, 12);
    // Every visible service row should be a ScrollNN — the well-known
    // `org.busx.Test` and the long name don't contain "scroll" so they must
    // have been filtered out.
    assert!(
        out.contains("org.busx.ScrollA"),
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
#[serial]
fn e2e_help_overlay_renders_over_service_list() {
    let _g = pid_filter().bind_to_scope();
    let out = run_scripted(harness().0, vec![key(KeyCode::Char('?'))], 80, 24);
    insta::assert_snapshot!(out);
}

/// `c` on the Detail screen (reached by drilling service → object → interface
/// → method → Call button) opens the copy-as popup, which renders the
/// currently-edited call as a `dbus-send` / `busctl` / `qdbus` / `gdbus`
/// command line. This exercises the production `tui::copy::generate` path
/// (including signature splitting and shell quoting) end-to-end through a
/// real interface, replacing the previously inlined
/// `tui::copy::split_signature_basics` / `quote_only_when_needed` unit tests.
///
/// Pressing Enter on the focused tool then fires `Effect::CopyToClipboard`,
/// which the production `run_effect` dispatches to `write_to_clipboard` —
/// here intercepted by the harness's mock `wl-copy` script rather than the
/// real compositor. The test asserts the captured stdin contains the
/// expected command line, proving the production clipboard write path
/// (spawn + stdin pipe) actually ran with the generated command.
#[test]
#[serial]
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
    keys.push(key(KeyCode::Down)); // row 0 (dbus-send) is unsupported for a{sv} → move to busctl
    keys.push(key(KeyCode::Enter)); // copy the busctl command

    // Hold the ClipboardMock past `run_scripted` so its tempdir (and log
    // file) survives for the contents() read below.
    let (h, clipboard) = harness();
    let out = run_scripted(h, keys, 100, 28);

    // The snapshot captures the rendered popup (commands included, with
    // non-deterministic socket paths / GUIDs masked by `pid_filter`); no
    // need for separate `contains` assertions on the rendered output.
    insta::assert_snapshot!(out);

    // The clipboard-mock log is the one thing the snapshot can't cover:
    // it verifies that pressing Enter actually fired the production
    // spawn path (`Effect::CopyToClipboard` → `run_effect` →
    // `write_to_clipboard` → mock `wl-copy`) and that the captured stdin
    // is the focused tool's full, untruncated command. TakeHints's
    // signature `a{sv}` is expressible by busctl but not dbus-send, so
    // the captured text must mention busctl + the real interface (no
    // dbus-send rendering could have made it through).
    let captured = clipboard.contents();
    assert!(
        captured.contains("busctl") && captured.contains("org.busx.Test"),
        "clipboard mock captured a busctl command for org.busx.Test: {captured:?}"
    );
    assert!(
        !captured.contains("dbus-send"),
        "dbus-send can't express a{{sv}} so the copy must not have fallen back to it: {captured:?}"
    );
}

/// Scripted keys that drill from the service list all the way into the
/// `org.busx.Test` Interface screen: filter `test`, Enter the only match,
/// Down to `/org/busx/Test` (the container paths sort ahead of it), Enter
/// twice (Objects → Interface, the latter via the single-interface auto-
/// skip). Each test below starts from this base so they only specify the
/// keys *after* reaching the Interface screen.
fn drill_into_test_interface() -> Vec<Msg> {
    let mut keys = vec![key(KeyCode::Char('/'))];
    for ch in "test".chars() {
        keys.push(key(KeyCode::Char(ch)));
    }
    keys.push(key(KeyCode::Enter)); // → Objects
    for _ in 0..3 {
        keys.push(key(KeyCode::Down)); // → /org/busx/Test
    }
    keys.push(key(KeyCode::Enter)); // → Interfaces (auto-skip) → Interface
    keys
}

/// Call a zero-arg method end-to-end: from the Interface screen, Down to
/// `MakeFd` (no args), Enter into the actions bar, Enter fire `Call` →
/// zero-input auto-fire skips Detail and lands on the Result screen. The
/// reply is a `h` (unix fd) → `/dev/null`, which exercises the fd-render
/// path (`value::fdinfo`) end-to-end.
#[test]
#[serial]
fn e2e_call_zero_arg_method() {
    let _g = pid_filter().bind_to_scope();
    let mut keys = drill_into_test_interface();
    keys.push(key(KeyCode::Down)); // methods[1] = Join
    keys.push(key(KeyCode::Down)); // methods[2] = BumpVolume
    keys.push(key(KeyCode::Down)); // methods[3] = MakeFd
    keys.push(key(KeyCode::Enter)); // enter the actions button bar
    keys.push(key(KeyCode::Enter)); // fire Call (zero-arg → auto-fire Result)
    let out = run_scripted(harness().0, keys, 80, 20);
    insta::assert_snapshot!(out);
}

/// Read a property end-to-end: Tab to the Properties column, Down to
/// `volume`, Enter into the actions bar, Enter fire `Get` → zero-input
/// auto-fire Result. The reply is `0.5` (the fixture's initial value).
#[test]
#[serial]
fn e2e_get_property() {
    let _g = pid_filter().bind_to_scope();
    let mut keys = drill_into_test_interface();
    keys.push(key(KeyCode::Tab)); // focus → Properties
    for _ in 0..3 {
        keys.push(key(KeyCode::Down)); // properties[3] = volume
    }
    keys.push(key(KeyCode::Enter)); // enter the actions button bar
    keys.push(key(KeyCode::Enter)); // fire Get (zero-input → auto-fire Result)
    let out = run_scripted(harness().0, keys, 80, 20);
    insta::assert_snapshot!(out);
}

/// Write a property end-to-end: Tab to Properties, Down to `volume`, Enter
/// into the actions bar, Down to `Set`, Enter push the Detail form, type
/// `1.5`, Tab to the trigger, Enter fire `Set` → Result. Exercises the
/// Detail typing path, `encode::parse` on `d` signature, the
/// `Effect::SetProperty` → `run_effect` branch, and the `ActionResult::Set`
/// render path.
#[test]
#[serial]
fn e2e_set_property() {
    let _g = pid_filter().bind_to_scope();
    let mut keys = drill_into_test_interface();
    keys.push(key(KeyCode::Tab)); // focus → Properties
    for _ in 0..3 {
        keys.push(key(KeyCode::Down)); // properties[3] = volume
    }
    keys.push(key(KeyCode::Enter)); // enter the actions button bar
    keys.push(key(KeyCode::Down)); // Set (buttons are Get, Set, Listen)
    keys.push(key(KeyCode::Enter)); // fire Set → push Detail (signature d)
    for ch in "1.5".chars() {
        keys.push(key(KeyCode::Char(ch))); // type the new value
    }
    keys.push(key(KeyCode::Tab)); // field → trigger
    keys.push(key(KeyCode::Enter)); // fire SetProperty → Result
    let out = run_scripted(harness().0, keys, 80, 20);
    insta::assert_snapshot!(out);
}

/// `Esc` pops the screen stack: from a Result screen (reached via the
/// zero-arg call above), Esc pops back to the Interface screen. Exercises
/// the pop path + nav field clearing (`nav.interface` cleared when the
/// Interface screen itself is popped, but here only the Result pops so
/// `nav` is preserved).
#[test]
#[serial]
fn e2e_esc_pops_result_back_to_interface() {
    let _g = pid_filter().bind_to_scope();
    let mut keys = drill_into_test_interface();
    keys.push(key(KeyCode::Down));
    keys.push(key(KeyCode::Down));
    keys.push(key(KeyCode::Down)); // MakeFd
    keys.push(key(KeyCode::Enter));
    keys.push(key(KeyCode::Enter)); // → Result
    keys.push(key(KeyCode::Esc)); // → pop back to Interface
    let out = run_scripted(harness().0, keys, 80, 20);
    insta::assert_snapshot!(out);
}

/// Listen on a method target end-to-end: drill into the Interface, Enter
/// the actions bar, Down to `Listen` (Methods column's second button),
/// Enter to arm. The production `Effect::Listen` with a
/// `ListenTarget::Method` spawns a dedicated connection and calls
/// `BecomeMonitor` on the bus; `Msg::ListenStarted` delivers the cancel
/// sender back to the Result screen. The app now mirrors `busctl monitor`
/// by waiting for the daemon's `NameLost(own_name)` confirmation before
/// forwarding any messages, so the BecomeMonitor lifecycle signals
/// (`NameAcquired`, the implicit `NameLost`, etc.) are filtered out and
/// the streaming Result stays empty when no real method calls happen
/// during the test window.
#[test]
#[serial]
fn e2e_listen_method_armed() {
    let _g = pid_filter().bind_to_scope();
    let mut keys = drill_into_test_interface();
    keys.push(key(KeyCode::Enter)); // enter the actions button bar
    keys.push(key(KeyCode::Down)); // Call → Listen
    keys.push(key(KeyCode::Enter)); // fire Listen (Method target) → Result streaming
    let out = run_scripted(harness().0, keys, 80, 20);
    insta::assert_snapshot!(out);
}

/// Listen on a property target end-to-end + Esc cancel: Tab to Properties,
/// Down to `volume`, Enter actions, Down × 2 to `Listen`, Enter to arm
/// (subscribes to the property's `PropertiesChanged`), then Esc to cancel.
/// Exercises `Effect::Listen` with `ListenTarget::Property`, the
/// `ListenStarted` → Result cancel-sender wiring, and the `Esc` cancel
/// path (dropping the sender stops the listen task). Snapshot is taken
/// after Esc, showing the Interface screen returned to.
#[test]
#[serial]
fn e2e_listen_property_armed_then_esc() {
    let _g = pid_filter().bind_to_scope();
    let mut keys = drill_into_test_interface();
    keys.push(key(KeyCode::Tab)); // focus → Properties
    for _ in 0..3 {
        keys.push(key(KeyCode::Down)); // volume
    }
    keys.push(key(KeyCode::Enter)); // enter the actions button bar
    keys.push(key(KeyCode::Down)); // Get → Set
    keys.push(key(KeyCode::Down)); // Set → Listen
    keys.push(key(KeyCode::Enter)); // fire Listen (Property target) → Result streaming
    keys.push(key(KeyCode::Esc)); // cancel listen + pop Result → back to Interface
    let out = run_scripted(harness().0, keys, 80, 20);
    insta::assert_snapshot!(out);
}

// ── Mouse click / scroll ──────────────────────────────────────────────
//
// Mouse coordinates are hardcoded for the fixed terminal sizes used by
// each test. In an 80×20 frame the service list's content rows start at
// y=2 (row 0 = breadcrumb, row 1 = block top border, row 2 = first data
// row). The block left border is at x=0, so any x ≥ 1 inside the NAME
// column lands on the row. The Interface screen's three-column layout
// puts methods at x≈1-58, properties below them, and the actions bar at
// x≈59-80. These positions are stable as long as the terminal size and
// the renderer's layout constants don't change.

/// A left-button click at (col, row).
fn mouse_down(col: u16, row: u16) -> Msg {
    Msg::Mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: col,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

/// A mouse-wheel scroll-down (one notch).
fn mouse_scroll_down(col: u16, row: u16) -> Msg {
    Msg::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: col,
        row,
        modifiers: KeyModifiers::NONE,
    })
}

/// Click a service row to drill into it. The first well-known name
/// (`org.busx.ScrollA`) is at row 0 and already selected by default, so
/// a single click triggers `handle_enter` (click an already-selected row
/// = Enter). It shares the testbus connection with `org.busx.Test`, so
/// object_tree returns the same set of paths.
#[test]
#[serial]
fn e2e_mouse_click_drills_into_service() {
    let _g = pid_filter().bind_to_scope();
    // row 0 (y=2) is the first well-known name (org.busx.ScrollA), which
    // is already selected → one click drills.
    let out = run_scripted(harness().0, vec![mouse_down(5, 2)], 80, 20);
    // After drilling we should be on the Objects screen (or auto-skipped
    // past it if only one path). Either way the breadcrumb shows the
    // service name.
    assert!(
        out.contains("ScrollA"),
        "drilled into ScrollA via mouse: {out}"
    );
    insta::assert_snapshot!(out);
}

/// Mouse scroll on the service list moves the selection down through
/// the list, exercising the `MouseEventKind::ScrollDown` path.
#[test]
#[serial]
fn e2e_mouse_scroll_on_service_list() {
    let _g = pid_filter().bind_to_scope();
    let keys: Vec<Msg> = (0..5).map(|_| mouse_scroll_down(5, 5)).collect();
    let out = run_scripted(harness().0, keys, 64, 8);
    insta::assert_snapshot!(out);
}

/// On the Interface screen, click a method row (selects + focuses
/// Methods), then click the `Call` action button (fires the action).
/// TakeHints has args (a{sv}) so Call pushes the Detail form — the
/// snapshot should show the Detail screen with the input field.
#[test]
#[serial]
fn e2e_mouse_click_method_and_call_button() {
    let _g = pid_filter().bind_to_scope();
    let mut keys = drill_into_test_interface();
    // method[0] (TakeHints) is at y=2 (first content row of the methods block).
    // Click once to select it.
    keys.push(mouse_down(5, 2));
    // The `Call` button is in the actions bar on the right side.
    // At 80 cols the actions block starts around x=59; the Call button
    // is row 0 of that block (y=2). Click it to fire the action.
    keys.push(mouse_down(65, 2));
    let out = run_scripted(harness().0, keys, 80, 20);
    assert!(
        out.contains("TakeHints") || out.contains("hints"),
        "reached Detail or Result via mouse click: {out}"
    );
    insta::assert_snapshot!(out);
}

/// Copy-as on a Get Result: drill into the Interface, Tab to Properties,
/// Down to `volume`, fire Get (zero-input auto-fire → Result), then press
/// `c` to open the copy-as popup. Covers `copy::generate`'s `Get` branch
/// for all four tools (dbus-send / busctl / qdbus / gdbus each have their
/// own `get-property` / `GetProperty` rendering).
#[test]
#[serial]
fn e2e_copy_as_get_result() {
    let _g = pid_filter().bind_to_scope();
    let mut keys = drill_into_test_interface();
    keys.push(key(KeyCode::Tab)); // focus → Properties
    for _ in 0..3 {
        keys.push(key(KeyCode::Down)); // volume
    }
    keys.push(key(KeyCode::Enter)); // enter the actions button bar
    keys.push(key(KeyCode::Enter)); // fire Get → Result
    keys.push(key(KeyCode::Char('c'))); // open copy-as popup
    let out = run_scripted(harness().0, keys, 100, 28);
    insta::assert_snapshot!(out);
}

/// Copy-as on a Set Detail: drill into the Interface, Tab to Properties,
/// Down to `volume`, Enter the actions bar, Down to `Set`, Enter push
/// Detail, then `c` to open the copy-as popup. Covers `copy::generate`'s
/// `Set` branch for all four tools.
#[test]
#[serial]
fn e2e_copy_as_set_detail() {
    let _g = pid_filter().bind_to_scope();
    let mut keys = drill_into_test_interface();
    keys.push(key(KeyCode::Tab)); // focus → Properties
    for _ in 0..3 {
        keys.push(key(KeyCode::Down)); // volume
    }
    keys.push(key(KeyCode::Enter)); // enter the actions button bar
    keys.push(key(KeyCode::Down)); // Get → Set
    keys.push(key(KeyCode::Enter)); // fire Set → push Detail
    keys.push(key(KeyCode::Char('c'))); // open copy-as popup
    let out = run_scripted(harness().0, keys, 100, 28);
    insta::assert_snapshot!(out);
}

/// Copy-as on a Listen Result: drill into the Interface, Enter actions,
/// Down to `Listen`, Enter fire Listen (Method target) → Result streaming,
/// then `c` to open the copy-as popup. Covers `copy::generate`'s `Listen`
/// branch (dbus-send → `dbus-monitor`, busctl → `busctl monitor`,
/// gdbus → `gdbus monitor`, qdbus unsupported).
#[test]
#[serial]
fn e2e_copy_as_listen_result() {
    let _g = pid_filter().bind_to_scope();
    let mut keys = drill_into_test_interface();
    keys.push(key(KeyCode::Enter)); // enter the actions button bar
    keys.push(key(KeyCode::Down)); // Call → Listen
    keys.push(key(KeyCode::Enter)); // fire Listen → Result streaming
    keys.push(key(KeyCode::Char('c'))); // open copy-as popup
    let out = run_scripted(harness().0, keys, 100, 28);
    insta::assert_snapshot!(out);
}
