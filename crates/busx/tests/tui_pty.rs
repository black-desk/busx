// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// // SPDX-License-Identifier: GPL-3.0-or-later

//! End-to-end TUI tests using tuiprobe.
//!
//! busx runs as a real subprocess (connected to a private testbus) inside a
//! PTY. Each test sends keyboard/mouse events, waits for the rendered output
//! to reach a known state (Cypress-style `wait_for_text`), then snapshots
//! the terminal screen.
//!
//! This replaces the old in-process `e2e_tests.rs` which used TestBackend +
//! ScriptedSource + fixed sleeps. The PTY approach tests the full code path
//! (main → CLI → crossterm → ratatui → render) and eliminates the 250ms-per-
//! key sleep that made the old suite take 77+ seconds.

use std::fs;
use std::time::Duration;

use portable_pty::CommandBuilder;
use tuiprobe::{KeyCode, MouseButton, ScrollDirection, TuiProbe, wait_for_snapshot};

// ── Helpers ──────────────────────────────────────────────────────────────

/// Spawn busx (TUI mode) connected to a private testbus.
fn spawn_busx(addr: &str, w: u16, h: u16) -> TuiProbe {
    let mut probe = TuiProbe::builder()
        .cols(w)
        .rows(h)
        .timeout(Duration::from_secs(5))
        .build()
        .expect("create probe");
    let mut cmd = CommandBuilder::new(busx_binary());
    cmd.arg("--address");
    cmd.arg(addr);
    probe.spawn(cmd).expect("spawn busx");
    probe
}

/// Spawn busx with a ClipboardMock that intercepts wl-copy/xclip/xsel.
/// The child's PATH is prepended with a tempdir of mock scripts, so
/// busx's `write_to_clipboard` spawns the mock instead of the real tool.
fn spawn_busx_with_clip(addr: &str, w: u16, h: u16) -> (TuiProbe, ClipboardMock) {
    let clip = ClipboardMock::new();
    let mut probe = TuiProbe::builder()
        .cols(w)
        .rows(h)
        .timeout(Duration::from_secs(5))
        .build()
        .expect("create probe");
    let mut cmd = CommandBuilder::new(busx_binary());
    cmd.arg("--address");
    cmd.arg(addr);
    cmd.env("PATH", clip.child_path());
    probe.spawn(cmd).expect("spawn busx");
    (probe, clip)
}

/// RAII guard that creates mock `wl-copy` / `xclip` / `xsel` scripts in a
/// tempdir. Each script appends its stdin to a shared log file. When busx's
/// `write_to_clipboard` spawns `wl-copy` (the first tool tried), the mock
/// captures the generated command line.
///
/// Unlike the old in-process ClipboardMock, this sets PATH only on the
/// **child process** via `CommandBuilder::env` — no `unsafe set_var`, no
/// `#[serial]` needed.
struct ClipboardMock {
    log_path: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

impl ClipboardMock {
    fn new() -> Self {
        let dir = tempfile::TempDir::new().expect("tempdir for clipboard mock");
        let log_path = dir.path().join("clipboard.log");
        // All three tools busx tries get the same script: append stdin to log.
        let script = format!("#!/bin/sh\ncat >> '{}'\n", log_path.display());
        for name in ["wl-copy", "xclip", "xsel"] {
            let path = dir.path().join(name);
            fs::write(&path, &script).expect("write mock script");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&path).unwrap().permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&path, perms).unwrap();
            }
        }
        ClipboardMock {
            log_path,
            _dir: dir,
        }
    }

    /// The PATH value to inject into the child process: mock-dir + current PATH.
    fn child_path(&self) -> String {
        format!(
            "{}:{}",
            self._dir.path().display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    /// Read everything busx has copied via the mock tools so far. Blocks
    /// briefly because `write_to_clipboard` does **not** wait on the spawned
    /// tool (it can daemonize) — give the mock a beat to finish draining.
    fn contents(&self) -> String {
        std::thread::sleep(Duration::from_millis(50));
        fs::read_to_string(&self.log_path).unwrap_or_default()
    }
}

/// Locate the compiled busx binary (tests run inside target/debug/deps/).
fn busx_binary() -> std::path::PathBuf {
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("busx")
}

/// Insta filter: mask PIDs, process comms, and socket paths so snapshots are
/// reproducible across runs.
fn pty_filter() -> insta::Settings {
    let mut s = insta::Settings::new();
    // PID column: anchor at `│` (left border), skip name_col + separator
    // (fixed width per terminal size), then match the PID column itself
    // (` *\d{1,7}` = right-aligned digits, exactly PID_W = 7 chars).
    // Replace with `  <PID>` (also 7 chars) so all columns past PID stay
    // aligned regardless of PID digit count (which varies local vs CI).
    //
    // Two patterns for the two terminal widths used in tests:
    //   64-wide: inner=62, name_w = 62-7-15-4 = 36 → prefix = │ + 38 chars
    //   80-wide: inner=78, name_w = 78-7-15-4 = 52 → prefix = │ + 54 chars
    s.add_filter(r"(?m)^(│.{38}) *\d{1,7}", "${1}  <PID>");
    s.add_filter(r"(?m)^(│.{54}) *\d{1,7}", "${1}  <PID>");
    // PROC column: process comm hash (PROC_W = 15 chars). Replace with
    // fixed-width `<PROC>         ` (15 chars) to preserve alignment.
    s.add_filter(r"tui_pty-[0-9a-f]+", "<PROC>         ");
    s.add_filter(r"busx-[0-9a-f]+", "<PROC>         ");
    // testbus socket path + GUID.
    s.add_filter(r#"unix:path=[^,\s"]+,guid=[^,\s"]+"#, "<SOCKET>");
    s.add_filter(r#"unix:abstract=[^,\s"]+,guid=[^,\s"]+"#, "<SOCKET>");
    s
}

/// Drill from the service list all the way into the org.busx.Test Interface
/// screen. Uses filter to narrow the service list, then navigates to
/// /org/busx/Test and enters it.
///
/// `suffix` disambiguates the wait-for snapshots by terminal size (the service
/// list and filtered objects list render differently at different sizes), so
/// callers pass e.g. `"80x20"` or `"100x28"`.
fn drill_to_interface(probe: &mut TuiProbe, suffix: &str) {
    wait_for_snapshot!(probe, format!("service_list_{}", suffix)).unwrap();

    // Filter to org.busx.Test.
    probe.send_key(KeyCode::Char('/')).unwrap();
    for ch in "test".chars() {
        probe.send_key(KeyCode::Char(ch)).unwrap();
    }
    probe.send_key(KeyCode::Enter).unwrap(); // → Objects

    // Navigate to /org/busx/Test (4th path after /, /org, /org/busx).
    wait_for_snapshot!(probe, format!("objects_list_test_filter_{}", suffix)).unwrap();
    for _ in 0..3 {
        probe.send_key(KeyCode::Down).unwrap();
    }
    probe.send_key(KeyCode::Enter).unwrap(); // → Interfaces (auto-skip) → Interface
}

// ── Service list ─────────────────────────────────────────────────────────

#[test]
fn service_list_renders() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 64, 12);

    wait_for_snapshot!(&mut probe, "service_list_64x12").unwrap();
    assert!(probe.contains("Services"));
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn down_arrow_moves_selection() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 64, 8);

    wait_for_snapshot!(&mut probe, "service_list_64x8").unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn service_list_scrolls_then_climbs() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 64, 8);

    wait_for_snapshot!(&mut probe, "service_list_64x8").unwrap();
    for _ in 0..10 {
        probe.send_key(KeyCode::Down).unwrap();
    }
    wait_for_snapshot!(&mut probe, "service_list_scrolled_scrollj").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

// ── Drill-down ───────────────────────────────────────────────────────────

#[test]
fn drill_into_objects_shows_paths() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 64, 12);

    wait_for_snapshot!(&mut probe, "service_list_64x12").unwrap();

    // Filter + Enter to drill into org.busx.Test's objects.
    probe.send_key(KeyCode::Char('/')).unwrap();
    for ch in "test".chars() {
        probe.send_key(KeyCode::Char(ch)).unwrap();
    }
    probe.send_key(KeyCode::Enter).unwrap();
    wait_for_snapshot!(&mut probe, "objects_list_test_filter_64x12").unwrap();

    assert!(probe.contains("/org/busx/Test"));
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn drill_to_interface_shows_methods_and_properties() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    drill_to_interface(&mut probe, "80x20");
    wait_for_snapshot!(&mut probe, "interface_loaded_80x20").unwrap();
    assert!(probe.contains("org.busx.Test"));
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn sub_object_has_distinct_volume() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    wait_for_snapshot!(&mut probe, "service_list_80x20").unwrap();

    // Drill into org.busx.Test, navigate to /org/busx/Test/sub.
    probe.send_key(KeyCode::Char('/')).unwrap();
    for ch in "test".chars() {
        probe.send_key(KeyCode::Char(ch)).unwrap();
    }
    probe.send_key(KeyCode::Enter).unwrap();
    wait_for_snapshot!(&mut probe, "objects_list_test_filter_80x20").unwrap();

    // Navigate to /sub (it's after /Test).
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Enter).unwrap();

    wait_for_snapshot!(&mut probe, "interface_loaded_sub_80x20").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

// ── Filter ───────────────────────────────────────────────────────────────

#[test]
fn filter_narrows_service_list() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 64, 12);

    wait_for_snapshot!(&mut probe, "service_list_64x12").unwrap();

    probe.send_key(KeyCode::Char('/')).unwrap();
    for ch in "scroll".chars() {
        probe.send_key(KeyCode::Char(ch)).unwrap();
    }
    wait_for_snapshot!(&mut probe, "service_list_filtered_scroll").unwrap();
    assert!(probe.contains("org.busx.ScrollA"));
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

// ── Help overlay ─────────────────────────────────────────────────────────

#[test]
fn help_overlay_renders() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 24);

    wait_for_snapshot!(&mut probe, "service_list_80x24").unwrap();
    probe.send_key(KeyCode::Char('?')).unwrap();
    wait_for_snapshot!(&mut probe, "help_overlay_80x24").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

// ── Call / Get / Set ─────────────────────────────────────────────────────

#[test]
fn call_zero_arg_method() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    drill_to_interface(&mut probe, "80x20");
    wait_for_snapshot!(&mut probe, "interface_loaded_80x20").unwrap();

    // Down to MakeFd (methods: TakeHints, Join, BumpVolume, MakeFd, ...).
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Enter).unwrap(); // enter button bar
    probe.send_key(KeyCode::Enter).unwrap(); // fire Call (zero-arg → Result)

    wait_for_snapshot!(&mut probe, "result_call_makefd_80x20").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn get_property() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    drill_to_interface(&mut probe, "80x20");
    wait_for_snapshot!(&mut probe, "interface_loaded_80x20").unwrap();

    // Tab to Properties column, Down to volume.
    probe.send_key(KeyCode::Tab).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Enter).unwrap(); // enter button bar
    probe.send_key(KeyCode::Enter).unwrap(); // fire Get → Result

    wait_for_snapshot!(&mut probe, "get_property_result").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn set_property() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    drill_to_interface(&mut probe, "80x20");
    wait_for_snapshot!(&mut probe, "interface_loaded_80x20").unwrap();

    // Tab to Properties, Down to volume.
    probe.send_key(KeyCode::Tab).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Enter).unwrap(); // enter button bar
    probe.send_key(KeyCode::Down).unwrap(); // Set button
    probe.send_key(KeyCode::Enter).unwrap(); // push Detail form

    // Type the new value.
    for ch in "1.5".chars() {
        probe.send_key(KeyCode::Char(ch)).unwrap();
    }
    probe.send_key(KeyCode::Tab).unwrap(); // field → trigger
    probe.send_key(KeyCode::Enter).unwrap(); // fire Set → Result

    wait_for_snapshot!(&mut probe, "result_set_80x20").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

// ── Esc ──────────────────────────────────────────────────────────────────

#[test]
fn esc_pops_result_back_to_interface() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    drill_to_interface(&mut probe, "80x20");
    wait_for_snapshot!(&mut probe, "interface_loaded_80x20").unwrap();

    // Call a zero-arg method to reach the Result screen, then Esc back.
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Enter).unwrap();
    probe.send_key(KeyCode::Enter).unwrap();
    wait_for_snapshot!(&mut probe, "result_call_makefd_80x20").unwrap();

    probe.send_key(KeyCode::Esc).unwrap();
    // Back on Interface screen — should show methods/properties again.
    // Focus is on the actions column (left over from firing the Call).
    wait_for_snapshot!(&mut probe, "interface_loaded_actions_focused_80x20").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

// ── Refresh (r key) ─────────────────────────────────────────────────────
//
// r-refresh tests have a subtlety: the data on screen doesn't change (the
// testbus fixture returns the same values), so wait_for_text can't detect
// completion by looking for "new" content. Instead we wait for the loading
// indicator to appear then disappear: busx sets `loading=true` on the
// screen (rendered as "(loading…)" in the title) while the refetch is
// in-flight, then clears it when the data arrives.

/// Wait for a refetch to complete: first sleep briefly so the loading
/// state has time to render, then poll until the "(loading…)" indicator
/// disappears from the screen.
fn wait_for_refresh_done(probe: &mut TuiProbe) {
    std::thread::sleep(Duration::from_millis(50));
    probe.wait_for(|s| !s.contains("loading")).unwrap();
}

#[test]
fn r_refreshes_service_list() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 64, 12);

    wait_for_snapshot!(&mut probe, "service_list_64x12").unwrap();
    probe.send_key(KeyCode::Char('r')).unwrap();
    wait_for_refresh_done(&mut probe);
    assert!(probe.contains("Services"));
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn r_refreshes_objects_list() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 64, 12);

    // Drill to the Objects screen.
    wait_for_snapshot!(&mut probe, "service_list_64x12").unwrap();
    probe.send_key(KeyCode::Char('/')).unwrap();
    for ch in "test".chars() {
        probe.send_key(KeyCode::Char(ch)).unwrap();
    }
    probe.send_key(KeyCode::Enter).unwrap();
    wait_for_snapshot!(&mut probe, "objects_list_test_filter_64x12").unwrap();

    // Press r to refetch the object tree.
    probe.send_key(KeyCode::Char('r')).unwrap();
    wait_for_refresh_done(&mut probe);
    assert!(probe.contains("/org/busx/Test/sub"));
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn r_refreshes_interface_properties() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    drill_to_interface(&mut probe, "80x20");
    wait_for_snapshot!(&mut probe, "interface_loaded_80x20").unwrap();

    // Press r to refetch the property-value snapshot (GetAll).
    probe.send_key(KeyCode::Char('r')).unwrap();
    wait_for_refresh_done(&mut probe);
    assert!(probe.contains("org.busx.Test"));
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

// ── Listen ───────────────────────────────────────────────────────────────

#[test]
fn listen_method_armed() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    drill_to_interface(&mut probe, "80x20");
    wait_for_snapshot!(&mut probe, "interface_loaded_80x20").unwrap();

    // Enter button bar, Down to Listen, fire.
    probe.send_key(KeyCode::Enter).unwrap();
    probe.send_key(KeyCode::Down).unwrap(); // Call → Listen
    probe.send_key(KeyCode::Enter).unwrap(); // fire Listen → Result streaming

    wait_for_snapshot!(&mut probe, "result_listen_method_80x20").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn listen_property_armed_then_esc() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    drill_to_interface(&mut probe, "80x20");
    wait_for_snapshot!(&mut probe, "interface_loaded_80x20").unwrap();

    // Tab to Properties, Down to volume.
    probe.send_key(KeyCode::Tab).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Enter).unwrap(); // enter button bar
    probe.send_key(KeyCode::Down).unwrap(); // Get → Set
    probe.send_key(KeyCode::Down).unwrap(); // Set → Listen
    probe.send_key(KeyCode::Enter).unwrap(); // fire Listen → Result

    wait_for_snapshot!(&mut probe, "result_listen_property_80x20").unwrap();
    probe.send_key(KeyCode::Esc).unwrap(); // cancel + pop

    // Back on Interface screen. Focus is on the actions column showing the
    // property actions (Get/Set/Listen) — distinct from the method-actions
    // variant in interface_loaded_actions_focused_80x20.
    wait_for_snapshot!(&mut probe, "interface_loaded_property_actions_focused_80x20").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

// ── Copy-as (clipboard command verification) ────────────────────────────
//
// These tests verify the **exact command text** busx generates for each
// copy-as tool (dbus-send / busctl / qdbus / gdbus). A ClipboardMock
// intercepts wl-copy's stdin, and we snapshot the full captured text —
// not just `contains` — so any formatting change (arg order, quoting,
// signature encoding) is caught.

/// Open the copy-as popup, navigate to the given tool row (0=dbus-send,
/// 1=busctl, 2=qdbus, 3=gdbus), press Enter to copy, and wait for the
/// "copied" status.
fn copy_tool(probe: &mut TuiProbe, tool_row: usize) {
    probe.send_key(KeyCode::Char('c')).unwrap();
    probe.wait_for_text("copy as").unwrap();
    for _ in 0..tool_row {
        probe.send_key(KeyCode::Down).unwrap();
    }
    probe.send_key(KeyCode::Enter).unwrap();
    probe.wait_for_text("copied").unwrap();
}

#[test]
fn copy_as_call_busctl() {
    // TakeHints has signature a{sv} — dbus-send can't express it, so the
    // popup shows "(unsupported)" for dbus-send. We copy the busctl command.
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let (mut probe, clip) = spawn_busx_with_clip(&bus.address, 100, 28);

    drill_to_interface(&mut probe, "100x28");
    probe.wait_for_text("volume").unwrap();

    // TakeHints (method 0), enter button bar, fire Call → Detail.
    probe.send_key(KeyCode::Enter).unwrap(); // enter button bar
    probe.send_key(KeyCode::Enter).unwrap(); // fire Call (a{sv} → Detail)

    // Copy busctl command (row 1).
    copy_tool(&mut probe, 1);
    insta::assert_snapshot!(clip.contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn copy_as_get_dbus_send() {
    // Get volume (signature d) — all tools can express. Copy the default
    // (dbus-send, row 0) from the Result screen.
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let (mut probe, clip) = spawn_busx_with_clip(&bus.address, 100, 28);

    drill_to_interface(&mut probe, "100x28");
    probe.wait_for_text("volume").unwrap();

    // Tab to Properties, Down to volume, Get → Result.
    probe.send_key(KeyCode::Tab).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Enter).unwrap(); // button bar
    probe.send_key(KeyCode::Enter).unwrap(); // fire Get → Result

    // Copy dbus-send command (row 0).
    copy_tool(&mut probe, 0);
    insta::assert_snapshot!(clip.contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn copy_as_set_dbus_send() {
    // Set volume = 1.5 (signature d) — copy from the Detail form (before
    // firing). Tests that copy-as reflects the typed value.
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let (mut probe, clip) = spawn_busx_with_clip(&bus.address, 100, 28);

    drill_to_interface(&mut probe, "100x28");
    probe.wait_for_text("volume").unwrap();

    // Tab to Properties, Down to volume, Set → Detail.
    probe.send_key(KeyCode::Tab).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Enter).unwrap(); // button bar
    probe.send_key(KeyCode::Down).unwrap(); // Set button
    probe.send_key(KeyCode::Enter).unwrap(); // push Detail

    // Type the value.
    for ch in "1.5".chars() {
        probe.send_key(KeyCode::Char(ch)).unwrap();
    }

    // Copy dbus-send command (row 0) from the Detail screen.
    copy_tool(&mut probe, 0);
    insta::assert_snapshot!(clip.contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn copy_as_listen_busctl() {
    // Listen on a signal — copy the busctl monitor command from Result.
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let (mut probe, clip) = spawn_busx_with_clip(&bus.address, 100, 28);

    drill_to_interface(&mut probe, "100x28");
    probe.wait_for_text("volume").unwrap();

    // Enter button bar, Down to Listen, fire → Result.
    probe.send_key(KeyCode::Enter).unwrap();
    probe.send_key(KeyCode::Down).unwrap(); // Call → Listen
    probe.send_key(KeyCode::Enter).unwrap(); // fire Listen → Result

    // Copy busctl command (row 1 — dbus-send can't express match rules).
    copy_tool(&mut probe, 1);
    insta::assert_snapshot!(clip.contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

// ── Mouse ────────────────────────────────────────────────────────────────

#[test]
fn mouse_click_drills_into_service() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    probe.wait_for_text("org.busx.ScrollA").unwrap();

    // Click on the first service row (y=2 = first content row after
    // breadcrumb + border). Row 0 already selected → click drills.
    probe.mouse_click(5, 2, MouseButton::Left).unwrap();

    // Should navigate to Objects screen.
    probe.wait_for_text("/org/busx/").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn mouse_scroll_on_service_list() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 64, 8);

    probe.wait_for_text("org.busx.ScrollA").unwrap();

    for _ in 0..5 {
        probe.mouse_scroll(5, 5, ScrollDirection::Down).unwrap();
    }
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}
