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

use std::time::Duration;

use portable_pty::CommandBuilder;
use tuiprobe::{KeyCode, KeyModifiers, MouseButton, ScrollDirection, TuiProbe};

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
    // Process comms FIRST (before the PID filter mangles the hex hash).
    s.add_filter(r"tui_pty-[0-9a-f]+", "<PROC>");
    s.add_filter(r"busx-[0-9a-f]+", "<PROC>");
    // testbus socket path + GUID.
    s.add_filter(r#"unix:path=[^,\s"]+,guid=[^,\s"]+"#, "<SOCKET>");
    s.add_filter(r#"unix:abstract=[^,\s"]+,guid=[^,\s"]+"#, "<SOCKET>");
    // PIDs last (4+ digits to avoid matching single-digit values in text).
    s.add_filter(r"\d{4,}", "<PID>");
    s
}

/// Drill from the service list all the way into the org.busx.Test Interface
/// screen. Uses filter to narrow the service list, then navigates to
/// /org/busx/Test and enters it.
fn drill_to_interface(probe: &mut TuiProbe) {
    probe.wait_for_text("org.busx.ScrollA").unwrap();

    // Filter to org.busx.Test.
    probe.send_key(KeyCode::Char('/')).unwrap();
    for ch in "test".chars() {
        probe.send_key(KeyCode::Char(ch)).unwrap();
    }
    probe.send_key(KeyCode::Enter).unwrap(); // → Objects

    // Navigate to /org/busx/Test (4th path after /, /org, /org/busx).
    probe.wait_for_text("/org").unwrap();
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

    probe.wait_for_text("org.busx.ScrollA").unwrap();
    assert!(probe.contains("Services"));
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn down_arrow_moves_selection() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 64, 8);

    probe.wait_for_text("org.busx.ScrollA").unwrap();
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

    probe.wait_for_text("org.busx.ScrollA").unwrap();
    for _ in 0..10 {
        probe.send_key(KeyCode::Down).unwrap();
    }
    probe.wait_for_text("ScrollJ").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

// ── Drill-down ───────────────────────────────────────────────────────────

#[test]
fn drill_into_objects_shows_paths() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 64, 12);

    probe.wait_for_text("org.busx.ScrollA").unwrap();

    // Filter + Enter to drill into org.busx.Test's objects.
    probe.send_key(KeyCode::Char('/')).unwrap();
    for ch in "test".chars() {
        probe.send_key(KeyCode::Char(ch)).unwrap();
    }
    probe.send_key(KeyCode::Enter).unwrap();
    probe.wait_for_text("/org/busx/Test").unwrap();

    assert!(probe.contains("/org/busx/Test"));
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn drill_to_interface_shows_methods_and_properties() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    drill_to_interface(&mut probe);
    probe.wait_for_text("volume").unwrap();
    assert!(probe.contains("org.busx.Test"));
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn sub_object_has_distinct_volume() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    probe.wait_for_text("org.busx.ScrollA").unwrap();

    // Drill into org.busx.Test, navigate to /org/busx/Test/sub.
    probe.send_key(KeyCode::Char('/')).unwrap();
    for ch in "test".chars() {
        probe.send_key(KeyCode::Char(ch)).unwrap();
    }
    probe.send_key(KeyCode::Enter).unwrap();
    probe.wait_for_text("/org/busx/Test").unwrap();

    // Navigate to /sub (it's after /Test).
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Enter).unwrap();

    probe.wait_for_text("volume").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

// ── Filter ───────────────────────────────────────────────────────────────

#[test]
fn filter_narrows_service_list() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 64, 12);

    probe.wait_for_text("org.busx.ScrollA").unwrap();

    probe.send_key(KeyCode::Char('/')).unwrap();
    for ch in "scroll".chars() {
        probe.send_key(KeyCode::Char(ch)).unwrap();
    }
    probe.wait_for_text("ScrollA").unwrap();
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

    probe.wait_for_text("org.busx.ScrollA").unwrap();
    probe.send_key(KeyCode::Char('?')).unwrap();
    probe.wait_for_text("keybindings").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

// ── Call / Get / Set ─────────────────────────────────────────────────────

#[test]
fn call_zero_arg_method() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    drill_to_interface(&mut probe);
    probe.wait_for_text("volume").unwrap();

    // Down to MakeFd (methods: TakeHints, Join, BumpVolume, MakeFd, ...).
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Enter).unwrap(); // enter button bar
    probe.send_key(KeyCode::Enter).unwrap(); // fire Call (zero-arg → Result)

    probe.wait_for_text("/dev/null").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn get_property() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    drill_to_interface(&mut probe);
    probe.wait_for_text("volume").unwrap();

    // Tab to Properties column, Down to volume.
    probe.send_key(KeyCode::Tab).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Enter).unwrap(); // enter button bar
    probe.send_key(KeyCode::Enter).unwrap(); // fire Get → Result

    probe.wait_for_text("0.5").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn set_property() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    drill_to_interface(&mut probe);
    probe.wait_for_text("volume").unwrap();

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

    probe.wait_for_text("ok").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

// ── Esc ──────────────────────────────────────────────────────────────────

#[test]
fn esc_pops_result_back_to_interface() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    drill_to_interface(&mut probe);
    probe.wait_for_text("volume").unwrap();

    // Call a zero-arg method to reach the Result screen, then Esc back.
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Enter).unwrap();
    probe.send_key(KeyCode::Enter).unwrap();
    probe.wait_for_text("/dev/null").unwrap();

    probe.send_key(KeyCode::Esc).unwrap();
    // Back on Interface screen — should show methods/properties again.
    probe.wait_for_text("volume").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

// ── Refresh (r key) ─────────────────────────────────────────────────────

#[test]
fn r_refreshes_service_list() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 64, 12);

    probe.wait_for_text("org.busx.ScrollA").unwrap();
    probe.send_key(KeyCode::Char('r')).unwrap();
    // After refresh, the list should still show services.
    probe.wait_for_text("org.busx.ScrollA").unwrap();
    assert!(probe.contains("Services"));
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

// ── Listen ───────────────────────────────────────────────────────────────

#[test]
fn listen_method_armed() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    drill_to_interface(&mut probe);
    probe.wait_for_text("volume").unwrap();

    // Enter button bar, Down to Listen, fire.
    probe.send_key(KeyCode::Enter).unwrap();
    probe.send_key(KeyCode::Down).unwrap(); // Call → Listen
    probe.send_key(KeyCode::Enter).unwrap(); // fire Listen → Result streaming

    probe.wait_for_text("listen").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

#[test]
fn listen_property_armed_then_esc() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 80, 20);

    drill_to_interface(&mut probe);
    probe.wait_for_text("volume").unwrap();

    // Tab to Properties, Down to volume.
    probe.send_key(KeyCode::Tab).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Down).unwrap();
    probe.send_key(KeyCode::Enter).unwrap(); // enter button bar
    probe.send_key(KeyCode::Down).unwrap(); // Get → Set
    probe.send_key(KeyCode::Down).unwrap(); // Set → Listen
    probe.send_key(KeyCode::Enter).unwrap(); // fire Listen → Result

    probe.wait_for_text("listen").unwrap();
    probe.send_key(KeyCode::Esc).unwrap(); // cancel + pop

    // Back on Interface screen.
    probe.wait_for_text("volume").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

    probe.send_key(KeyCode::Char('q')).unwrap();
}

// ── Copy-as popup ────────────────────────────────────────────────────────

#[test]
fn copy_as_popup_on_interface() {
    let _g = pty_filter().bind_to_scope();
    let bus = testbus::bus_owned();
    let mut probe = spawn_busx(&bus.address, 100, 28);

    drill_to_interface(&mut probe);
    probe.wait_for_text("volume").unwrap();

    // Enter button bar, fire Call on TakeHints (a{sv}).
    probe.send_key(KeyCode::Enter).unwrap();
    probe.send_key(KeyCode::Enter).unwrap(); // push Detail

    // Open copy-as popup.
    probe.send_key(KeyCode::Char('c')).unwrap();
    probe.wait_for_text("busctl").unwrap();
    insta::assert_snapshot!(probe.screen_contents());

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
