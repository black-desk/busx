// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Public-interface tests: real busx in a PTY connected to an isolated bus.
//! Wait for observable outcomes, independent of visual snapshots. Snapshots
//! cover representative layouts; D-Bus and clipboard checks cover effects.

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use portable_pty::CommandBuilder;
use tuiprobe::{KeyCode, KeyModifiers, MouseButton, ScrollDirection, TuiProbe};
use zbus::blocking::{Connection, Proxy, connection::Builder};

const SERVICE: &str = "org.busx.Test";
const PATH: &str = "/org/busx/Test";

fn spawn(addr: &str, cols: u16, rows: u16, clip: Option<&ClipboardMock>) -> TuiProbe {
    spawn_with_editor(addr, cols, rows, clip, None)
}

fn spawn_with_editor(
    addr: &str,
    cols: u16,
    rows: u16,
    clip: Option<&ClipboardMock>,
    editor: Option<&Path>,
) -> TuiProbe {
    let mut probe = TuiProbe::builder()
        .cols(cols)
        .rows(rows)
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let mut cmd = CommandBuilder::new(assert_cmd::cargo::cargo_bin!("busx"));
    cmd.args(["--address", addr]);
    cmd.env_remove("VISUAL");
    cmd.env("EDITOR", editor.unwrap_or_else(|| Path::new("true")));
    if let Some(clip) = clip {
        cmd.env("PATH", clip.child_path());
    }
    probe.spawn(cmd).unwrap();
    probe
}

fn key(probe: &mut TuiProbe, key: KeyCode) {
    probe.send_key(key).unwrap();
}
fn text(probe: &mut TuiProbe, text: &str) {
    probe.send_text(text).unwrap();
}
fn wait(probe: &mut TuiProbe, needle: &str) {
    probe.wait_for(|screen| screen.contains(needle)).unwrap();
}
fn services(probe: &mut TuiProbe) {
    probe
        .wait_for(|s| {
            s.contains("Services (")
                && s.contains("Name")
                && !s.contains("loading")
                && !s.contains("Bus · current tab")
        })
        .unwrap();
}

fn location(probe: &TuiProbe, needle: &str) -> (u16, u16) {
    probe
        .screen_contents()
        .lines()
        .enumerate()
        .find_map(|(row, line)| {
            line.find(needle)
                .map(|offset| (line[..offset].chars().count() as u16, row as u16))
        })
        .unwrap_or_else(|| panic!("missing {needle:?}\n{}", probe.screen_contents()))
}

fn click_up(probe: &mut TuiProbe, column: u16) {
    let (x, y) = location(probe, " ^ ");
    assert_eq!((x, y), (0, 1));
    probe.mouse_click(x + column, y, MouseButton::Left).unwrap();
}

fn custom_bus(probe: &mut TuiProbe, address: &str) {
    text(probe, "b");
    wait(probe, "Bus · current tab");
    // These scenarios start on a custom bus, so its address already has focus.
    probe
        .send_key_with_mods(KeyCode::Char('u'), KeyModifiers::CTRL)
        .unwrap();
    text(probe, address);
    key(probe, KeyCode::Enter);
}
fn filter(probe: &mut TuiProbe, query: &str) {
    text(probe, "/");
    text(probe, query);
    wait(probe, &format!("Filter /{query}"));
}
fn open_service(probe: &mut TuiProbe, service: &str) {
    services(probe);
    filter(probe, service);
    key(probe, KeyCode::Enter);
    probe
        .wait_for(|s| s.contains("Navigation") && !s.contains("loading"))
        .unwrap();
}
fn open_interface(probe: &mut TuiProbe) {
    open_interface_for(probe, SERVICE);
}
fn open_interface_for(probe: &mut TuiProbe, service: &str) {
    open_service(probe, service);
    filter(probe, SERVICE);
    key(probe, KeyCode::Down); // first matching object -> its interface
    key(probe, KeyCode::Enter);
    probe
        .wait_for(|s| s.contains("Methods") && s.contains("counts") && !s.contains("loading"))
        .unwrap();
}
fn member(probe: &mut TuiProbe, name: &str) {
    filter(probe, name);
    key(probe, KeyCode::Enter);
    probe
        .wait_for(|s| s.contains(name) && (s.contains("NORMAL") || s.contains("Enter run")))
        .unwrap();
}
fn method(probe: &mut TuiProbe, name: &str) {
    text(probe, "l"); // initial category is Properties
    member(probe, name);
}
fn edit(probe: &mut TuiProbe, value: &str) {
    text(probe, "i");
    wait(probe, "INSERT");
    text(probe, value);
    key(probe, KeyCode::Esc);
    wait(probe, "NORMAL");
}
fn call(probe: &mut TuiProbe) {
    key(probe, KeyCode::Enter);
    wait(probe, "SUCCESS");
}
fn connection(address: &str) -> Connection {
    Builder::address(address).unwrap().build().unwrap()
}
fn target(conn: &Connection) -> Proxy<'_> {
    Proxy::new(conn, SERVICE, PATH, SERVICE).unwrap()
}
#[track_caller]
fn poll(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "observable effect did not arrive before deadline"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn close_last(probe: &mut TuiProbe) {
    text(probe, "q");
    poll(|| !probe.is_running());
    assert!(probe.wait_exit().unwrap().success());
}
// Normalize only service-table metadata, preserving each terminal column.
// Do not apply PID regexes to arbitrary frames (property values are numbers too).
fn snapshot(name: &str, probe: &TuiProbe) {
    let screen = probe.screen_contents();
    let mut lines: Vec<String> = screen.lines().map(str::to_owned).collect();
    let dropdown_column = lines.iter().find_map(|line| {
        line.find("┌Bus · current tab")
            .map(|x| line[..x].chars().count())
    });
    if let Some((title, width)) = lines.iter().enumerate().find_map(|(row, line)| {
        let width = line.chars().count();
        (line.starts_with("┌Services ") && width >= 52).then_some((row, width))
    }) {
        // The right-aligned dropdown can hide the metadata column headers.
        let pid_end = width - 18;
        for line in lines
            .iter_mut()
            .skip(title + 2)
            .take_while(|line| !line.starts_with('└') || !line.ends_with('┘'))
        {
            let mut cells: Vec<char> = line.chars().collect();
            // All metadata is hidden on rows covered by the menu. Preserve
            // its text and border instead of normalizing those cells.
            if cells.len() < pid_end + 15
                || dropdown_column.is_some_and(|x| matches!(cells.get(x), Some('┌' | '│' | '└')))
            {
                continue;
            }
            let process: String = cells[pid_end + 2..pid_end + 15].iter().collect();
            let fixture = process.trim_start().starts_with("tui_pty-");
            if cells.get(3).is_none_or(|ch| ch.is_whitespace()) && !fixture {
                continue;
            }
            cells[pid_end - 7..pid_end].copy_from_slice(&"  <PID>".chars().collect::<Vec<_>>());
            if fixture {
                cells[pid_end + 2..pid_end + 15]
                    .copy_from_slice(&"    <FIXTURE>".chars().collect::<Vec<_>>());
            }
            *line = cells.into_iter().collect();
        }
    }
    insta::assert_snapshot!(name, lines.join("\n"));
}

struct ClipboardMock {
    log_path: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

struct EditorMock {
    path: std::path::PathBuf,
    output: std::path::PathBuf,
    _dir: tempfile::TempDir,
}

impl EditorMock {
    fn new() -> Self {
        let dir = tempfile::TempDir::new().expect("tempdir for editor mock");
        let path = dir.path().join("editor");
        let output = dir.path().join("result.txt");
        let script = format!("#!/bin/sh\ncat \"$1\" > '{}'\n", output.display());
        fs::write(&path, script).expect("write editor mock");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&path, perms).unwrap();
        }
        Self {
            path,
            output,
            _dir: dir,
        }
    }

    fn contents(&self) -> String {
        fs::read_to_string(&self.output).expect("editor output")
    }
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

    /// Read the clipboard mock log; callers poll until the expected copy lands.
    fn contents(&self) -> String {
        fs::read_to_string(&self.log_path).unwrap_or_default()
    }
}

#[test]
fn service_list_layout_and_small_terminal() {
    let bus = testbus::bus_owned();
    let mut p = spawn(&bus.address, 80, 24, None);
    services(&mut p);
    assert!(
        p.screen_contents()
            .lines()
            .next()
            .unwrap()
            .contains("1 Custom ×")
    );
    snapshot("services_80x24", &p);
    p.resize(64, 12).unwrap();
    p.wait_for(|s| s.lines().count() == 12 && s.contains("Name") && s.contains("? help"))
        .unwrap();
    snapshot("services_64x12", &p);
    p.resize(64, 8).unwrap();
    wait(&mut p, "Terminal too small");
    snapshot("terminal_too_small", &p);
    close_last(&mut p);
}

#[test]
fn service_filter_scroll_refresh_and_mouse() {
    let bus = testbus::bus_owned();
    let conn = connection(&bus.address);
    let mut p = spawn(&bus.address, 64, 12, None);
    services(&mut p);
    filter(&mut p, "Scroll");
    p.wait_for(|s| s.contains("ScrollA") && !s.contains("org.busx.Test"))
        .unwrap();
    key(&mut p, KeyCode::Esc);
    p.wait_for(|s| !s.contains("Filter /")).unwrap();
    text(&mut p, "G");
    wait(&mut p, "> :1.");
    text(&mut p, "gg");
    wait(&mut p, "> org.busx.ScrollA");
    p.mouse_scroll(5, 5, ScrollDirection::Down).unwrap();
    wait(&mut p, "> org.busx.ScrollB");
    conn.request_name("org.busx.AddedAfterStartup").unwrap();
    text(&mut p, "r");
    wait(&mut p, "org.busx.AddedAfterStartup");
    text(&mut p, "gg");
    wait(&mut p, "> org.busx.AddedAfterStartup");
    p.mouse_click(5, 3, MouseButton::Left).unwrap();
    wait(&mut p, "Navigation");
}

#[test]
fn interface_layout_and_subobject_navigation() {
    let bus = testbus::bus_owned();
    let mut p = spawn(&bus.address, 80, 24, None);
    open_interface(&mut p);
    snapshot("interface_80x24", &p);
    text(&mut p, "n");
    filter(&mut p, "/sub");
    key(&mut p, KeyCode::Down);
    key(&mut p, KeyCode::Enter);
    probe_property(&mut p, "volume");
    wait(&mut p, "Current  0");
    assert!(p.contains("/sub"));
}

fn probe_property(p: &mut TuiProbe, name: &str) {
    member(p, name);
}

#[test]
fn narrow_interface_and_help_layout() {
    let bus = testbus::bus_owned();
    let mut p = spawn(&bus.address, 64, 16, None);
    open_interface(&mut p);
    snapshot("interface_64x16", &p);
    text(&mut p, "n");
    wait(&mut p, "Navigation");
    assert!(!p.contains("Current:"));
    p.resize(80, 24).unwrap();
    text(&mut p, "?");
    wait(&mut p, "busx · keyboard");
    snapshot("help_80x24", &p);
    key(&mut p, KeyCode::Esc);
    wait(&mut p, "Navigation");
}

#[test]
fn call_zero_arg_method_and_open_result_in_editor() {
    let bus = testbus::bus_owned();
    let mut p = spawn(&bus.address, 80, 24, None);
    open_interface(&mut p);
    method(&mut p, "MakeFd");
    call(&mut p);
    wait(&mut p, "/dev/null");
    snapshot("method_result_80x24", &p);
    text(&mut p, "j");
    wait(&mut p, "Enter open in editor");
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "Opened in editor");
    assert!(p.contains("SUCCESS"));
    close_last(&mut p);
}

#[test]
fn result_opens_in_editor() {
    let bus = testbus::bus_owned();
    let mut p = spawn(&bus.address, 80, 24, None);
    open_interface(&mut p);
    method(&mut p, "LongString");
    call(&mut p);
    wait(&mut p, "the quick brown fox");
    text(&mut p, "j");
    wait(&mut p, "Enter open in editor");
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "Opened in editor");
    close_last(&mut p);
}

#[test]
fn editor_formats_nested_results() {
    let bus = testbus::bus_owned();
    let editor = EditorMock::new();
    let mut p = spawn_with_editor(&bus.address, 80, 24, None, Some(&editor.path));
    open_interface(&mut p);
    probe_property(&mut p, "counts");
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "SUCCESS");
    text(&mut p, "j");
    wait(&mut p, "Enter open in editor");
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "Opened in editor");
    assert_eq!(editor.contents(), "{\n  1: 10,\n  2: 20\n}");
    close_last(&mut p);
}

#[test]
fn property_read_write_and_refresh() {
    let bus = testbus::bus_owned();
    let conn = connection(&bus.address);
    let proxy = target(&conn);
    let mut p = spawn(&bus.address, 80, 24, None);
    open_interface(&mut p);
    probe_property(&mut p, "volume");
    wait(&mut p, "Current  0.5");
    edit(&mut p, "1.5");
    assert_eq!(proxy.get_property::<f64>("volume").unwrap(), 0.5);
    key(&mut p, KeyCode::Down);
    wait(&mut p, "Enter run");
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "SUCCESS");
    assert_eq!(proxy.get_property::<f64>("volume").unwrap(), 1.5);
    text(&mut p, "l");
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "1.5");
    proxy.set_property("volume", 2.75f64).unwrap();
    text(&mut p, "r");
    wait(&mut p, "Current  2.75");
    snapshot("property_editor_80x24", &p);
}

#[test]
fn insert_mode_and_directional_focus_do_not_execute_calls() {
    let bus = testbus::bus_owned();
    let mut p = spawn(&bus.address, 80, 24, None);
    open_interface(&mut p);
    method(&mut p, "Join");
    edit(&mut p, "1 qtjk?");
    assert!(p.contains("1 qtjk?"));
    assert!(!p.contains("SUCCESS"));
    key(&mut p, KeyCode::Down);
    wait(&mut p, "Enter run");
    key(&mut p, KeyCode::Up);
    wait(&mut p, "NORMAL");
    assert!(p.contains("1 qtjk?"));
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "Enter run");
    assert!(!p.contains("SUCCESS"));
    call(&mut p);
    wait(&mut p, r#""qtjk?""#);
}

#[test]
fn tabs_preserve_inputs_results_filters_and_close_independently() {
    let bus = testbus::bus_owned();
    let mut p = spawn(&bus.address, 80, 24, None);
    open_interface(&mut p);
    method(&mut p, "Join");
    edit(&mut p, "1 remembered");
    key(&mut p, KeyCode::Enter);
    call(&mut p);
    wait(&mut p, r#""remembered""#);
    text(&mut p, "t");
    services(&mut p);
    filter(&mut p, "ScrollC");
    wait(&mut p, "org.busx.ScrollC");
    // Mouse selection also works while the other tab owns a filter input.
    let (x, y) = location(&p, "1 🔍");
    p.mouse_click(x, y, MouseButton::Left).unwrap();
    wait(&mut p, "SUCCESS");
    assert!(p.contains("1 remembered"));
    text(&mut p, "gt");
    wait(&mut p, "Filter /ScrollC");
    key(&mut p, KeyCode::Esc);
    p.wait_for(|s| !s.contains("Filter /")).unwrap();
    text(&mut p, "t");
    services(&mut p);
    text(&mut p, "1gt");
    wait(&mut p, "SUCCESS");
    text(&mut p, "gT");
    wait(&mut p, "3 Custom");
    text(&mut p, "q");
    wait(&mut p, "2 Custom");
    text(&mut p, "q");
    wait(&mut p, "SUCCESS");
    close_last(&mut p);
}

#[test]
fn mouse_new_and_close_tab_buttons() {
    let bus = testbus::bus_owned();
    let mut p = spawn(&bus.address, 80, 24, None);
    services(&mut p);
    p.mouse_click(78, 0, MouseButton::Left).unwrap();
    wait(&mut p, "2 Custom");
    // Close the first, inactive tab via its visible ×, keeping tab 2 active.
    let header = p.screen_contents().lines().next().unwrap().to_string();
    let x = header.chars().position(|ch| ch == '×').unwrap() as u16;
    p.mouse_click(x, 0, MouseButton::Left).unwrap();
    p.wait_for(|s| !s.lines().next().unwrap().contains("2 Custom"))
        .unwrap();
    assert!(p.is_running());
    close_last(&mut p);
}

#[test]
fn tab_strip_overflow_keeps_active_tab_visible() {
    let bus = testbus::bus_owned();
    let mut p = spawn(&bus.address, 64, 12, None);
    services(&mut p);
    for _ in 0..9 {
        text(&mut p, "t");
    }
    wait(&mut p, "10 Custom");
    services(&mut p);
    let header = p.screen_contents().lines().next().unwrap().to_string();
    assert!(header.contains("7 Custom ×"), "{header}");
    assert!(header.contains("8 Custom ×"), "{header}");
    assert!(header.contains("9 Custom ×"), "{header}");
    assert!(header.contains("10 Custom ×"), "{header}");
    snapshot("tabs_overflow_64x12", &p);
    text(&mut p, "1gt");
    wait(&mut p, "1 Custom");
    text(&mut p, "gT");
    wait(&mut p, "10 Custom");
    p.resize(32, 12).unwrap();
    p.wait_for(|screen| {
        let header = screen.lines().next().unwrap();
        header.chars().count() <= 32
            && header.contains("10")
            && header.matches('×').count() >= 1
            && header.contains('+')
    })
    .unwrap();
    p.resize(100, 12).unwrap();
    p.wait_for(|screen| {
        let header = screen.lines().next().unwrap();
        header.contains("10 Custom") && header.matches('×').count() == 7
    })
    .unwrap();
}

#[test]
fn browser_titles_shorten_names_disambiguate_and_keep_mouse_targets_aligned() {
    let bus = testbus::bus_owned();
    let _other = Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.example.Test")
        .unwrap()
        .serve_at(PATH, ForeignInterface)
        .unwrap()
        .build()
        .unwrap();
    let mut p = spawn(&bus.address, 100, 24, None);
    open_service(&mut p, SERVICE);
    let header = p.screen_contents().lines().next().unwrap().to_string();
    assert!(
        header.contains('🔍') && header.contains("…Test"),
        "{header}"
    );
    assert!(
        !header.contains("org.busx.Test") && !header.contains("· Custom"),
        "{header}"
    );
    text(&mut p, "t");
    open_service(&mut p, "org.example.Test");
    p.wait_for(|screen| {
        let header = screen.lines().next().unwrap();
        header.contains("…busx.Test") && header.contains("…example.Test")
    })
    .unwrap();
    let header = p.screen_contents().lines().next().unwrap().to_string();
    assert!(header.contains("…busx.Test ×"), "{header}");
    assert!(header.contains("…example.Test ×"), "{header}");
    snapshot("browser_tab_titles_100x24", &p);
    // The emoji occupies two terminal columns. Closing the inactive tab must
    // still hit its own close button and keep the other browser open.
    let header = p.screen_contents().lines().next().unwrap().to_string();
    let close_x = header.chars().position(|ch| ch == '×').unwrap() as u16;
    p.mouse_click(close_x, 0, MouseButton::Left).unwrap();
    p.wait_for(|screen| {
        let header = screen.lines().next().unwrap();
        header.contains("…Test")
            && !header.contains("…example.Test")
            && !header.contains("…busx.Test")
    })
    .unwrap();
    filter(&mut p, "org.example.UnrelatedNamespace");
    key(&mut p, KeyCode::Down);
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "foreign-interface");
    assert!(p.contains("org.example.Test"));
    text(&mut p, "n");
    wait(&mut p, "h/l fold");
    p.resize(64, 12).unwrap();
    p.wait_for(|screen| {
        screen.lines().count() == 12 && screen.lines().next().unwrap().contains('🔍')
    })
    .unwrap();
    text(&mut p, "t");
    services(&mut p);
    wait(&mut p, "2 Custom");
    let (x, y) = location(&p, "1 🔍");
    p.mouse_click(x, y, MouseButton::Left).unwrap();
    wait(&mut p, "Navigation");
    assert!(p.screen_contents().lines().next().unwrap().contains('🔍'));
    text(&mut p, "q");
    services(&mut p);
    wait(&mut p, "1 Custom");
    close_last(&mut p);
}

#[test]
fn watch_titles_mark_listening_and_disambiguate_same_named_members() {
    let bus = testbus::bus_owned();
    let mut p = spawn(&bus.address, 100, 24, None);
    start_property_watch(&mut p);
    let header = p.screen_contents().lines().next().unwrap().to_string();
    assert!(
        header.contains('👁') && header.contains("…volume"),
        "{header}"
    );
    assert!(
        !header.contains("Watch") && !header.contains("· Custom"),
        "{header}"
    );
    text(&mut p, "p");
    wait(&mut p, "PAUSED DISPLAY");
    let header = p.screen_contents().lines().next().unwrap().to_string();
    assert!(
        header.contains('👁') && header.contains("…volume"),
        "{header}"
    );
    text(&mut p, "t");
    start_property_watch_for(&mut p, "org.busx.ScrollA");
    p.wait_for(|screen| {
        let header = screen.lines().next().unwrap();
        header.matches('👁').count() == 2
            && header.contains("…Test:volume")
            && header.contains("…ScrollA:volume")
    })
    .unwrap();
    let header = p.screen_contents().lines().next().unwrap().to_string();
    assert!(header.contains("…Test:volume ×"), "{header}");
    assert!(header.contains("…ScrollA:volume ×"), "{header}");
    snapshot("watch_tab_titles_100x24", &p);
    let header = p.screen_contents().lines().next().unwrap().to_string();
    let close_x = header.chars().position(|ch| ch == '×').unwrap() as u16;
    p.mouse_click(close_x, 0, MouseButton::Left).unwrap();
    p.wait_for(|screen| {
        let header = screen.lines().next().unwrap();
        header.matches('👁').count() == 1
            && header.contains("…volume")
            && !header.contains("ScrollA:volume")
    })
    .unwrap();
    close_last(&mut p);
}

#[test]
fn session_and_system_tabs_keep_independent_buses_and_watches() {
    let session = testbus::bus_owned();
    let system = testbus::bus_owned();
    let session_conn = connection(&session.address);
    let system_conn = connection(&system.address);
    session_conn.request_name("org.busx.SessionOnly").unwrap();
    system_conn.request_name("org.busx.SystemOnly").unwrap();
    let session_proxy = target(&session_conn);
    let system_proxy = target(&system_conn);
    // Include the test client's own property-cache subscription in the baseline.
    assert_eq!(session_proxy.get_property::<f64>("volume").unwrap(), 0.5);
    let baseline = property_subscriptions(&session_conn);
    let mut p = TuiProbe::new(100, 28).unwrap();
    let mut cmd = CommandBuilder::new(assert_cmd::cargo::cargo_bin!("busx"));
    cmd.arg("--user");
    cmd.env("DBUS_SESSION_BUS_ADDRESS", &session.address);
    cmd.env("DBUS_SYSTEM_BUS_ADDRESS", &system.address);
    p.spawn(cmd).unwrap();
    services(&mut p);
    wait(&mut p, "1 Session");
    assert!(p.contains("org.busx.SessionOnly"));
    assert!(!p.contains("org.busx.SystemOnly"));
    start_property_watch(&mut p);
    poll(|| property_subscriptions(&session_conn) == baseline + 1);
    text(&mut p, "t");
    services(&mut p);
    wait(&mut p, "2 Session");
    // The header and b both open the same current-tab bus selector.
    let (x, y) = location(&p, "Bus▾");
    p.mouse_click(x + 3, y, MouseButton::Left).unwrap();
    wait(&mut p, "Bus · current tab");
    let (x, y) = location(&p, "System");
    p.mouse_click(x, y, MouseButton::Left).unwrap();
    services(&mut p);
    wait(&mut p, "2 System");
    assert!(p.contains("org.busx.SystemOnly"));
    assert!(!p.contains("org.busx.SessionOnly"));
    open_interface(&mut p);
    probe_property(&mut p, "volume");
    edit(&mut p, "8.5");
    key(&mut p, KeyCode::Down);
    call(&mut p);
    assert_eq!(system_proxy.get_property::<f64>("volume").unwrap(), 8.5);
    assert_eq!(session_proxy.get_property::<f64>("volume").unwrap(), 0.5);
    text(&mut p, "c");
    wait(&mut p, "dbus-send --system");
    key(&mut p, KeyCode::Esc);
    p.wait_for(|s| !s.contains("Copy command · Esc close"))
        .unwrap();
    text(&mut p, "t");
    services(&mut p);
    wait(&mut p, "3 System");
    text(&mut p, "b");
    wait(&mut p, "Bus · current tab");
    key(&mut p, KeyCode::Up); // System -> Session
    key(&mut p, KeyCode::Enter);
    services(&mut p);
    wait(&mut p, "3 Session");
    open_service(&mut p, SERVICE);
    p.wait_for(|screen| {
        let header = screen.lines().next().unwrap();
        header.contains("…Test · System") && header.contains("…Test · Session")
    })
    .unwrap();
    text(&mut p, "q");
    wait(&mut p, "SUCCESS");
    system_proxy.set_property("volume", 9.5f64).unwrap();
    session_proxy.set_property("volume", 6.25f64).unwrap();
    text(&mut p, "gt");
    wait(&mut p, "6.25");
    wait(&mut p, "1 events");
    assert!(!p.contains("9.5"));
    assert!(!p.contains("Bus▾"));
    wait(&mut p, "6.25");
    poll(|| property_subscriptions(&session_conn) == baseline + 1);
    text(&mut p, "gt");
    key(&mut p, KeyCode::Esc);
    wait(&mut p, "h/l fold");
    key(&mut p, KeyCode::Esc);
    services(&mut p);
    text(&mut p, "r");
    wait(&mut p, "org.busx.SystemOnly");
    assert!(!p.contains("org.busx.SessionOnly"));
}

#[test]
fn custom_bus_switch_starts_from_services_and_reloads_after_success() {
    let first = testbus::bus_owned();
    let second = testbus::bus_owned();
    let other = connection(&second.address);
    other.request_name("org.busx.SecondBus").unwrap();
    let mut p = spawn(&first.address, 80, 24, None);
    services(&mut p);
    custom_bus(&mut p, &second.address);
    services(&mut p);
    wait(&mut p, "org.busx.SecondBus");
}

#[test]
fn bus_picker_keyboard_selection_and_cancel_preserve_the_tab() {
    let bus = testbus::bus_owned();
    let mut p = spawn(&bus.address, 64, 16, None);
    services(&mut p);
    filter(&mut p, "ScrollC");
    key(&mut p, KeyCode::Esc);
    p.wait_for(|s| !s.contains("Filter /")).unwrap();
    text(&mut p, "b");
    wait(&mut p, "Bus · current tab");
    p.send_key_with_mods(KeyCode::Char('u'), KeyModifiers::CTRL)
        .unwrap();
    text(&mut p, "unix:path=/tmp/busx-example");
    wait(&mut p, "unix:path=/tmp/busx-example");
    key(&mut p, KeyCode::Down); // custom -> session
    p.wait_for(|s| s.lines().any(|line| line.contains("> Session")))
        .unwrap();
    key(&mut p, KeyCode::Down);
    p.wait_for(|s| s.lines().any(|line| line.contains("> System")))
        .unwrap();
    snapshot("bus_picker_64x16", &p);
    key(&mut p, KeyCode::Esc);
    services(&mut p);
    wait(&mut p, "1 Custom");
    text(&mut p, "b");
    wait(&mut p, "Bus · current tab");
    p.send_key_with_mods(KeyCode::Char('u'), KeyModifiers::CTRL)
        .unwrap();
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "Enter a D-Bus address");
    key(&mut p, KeyCode::Esc);
    services(&mut p);
    close_last(&mut p);
}

#[test]
fn bus_picker_dropdown_overlays_the_page_and_outside_clicks_preserve_focus() {
    let assert_menu_position = |p: &TuiProbe| {
        let (trigger_x, trigger_y) = location(p, "Bus▾");
        let (menu_x, menu_y) = location(p, "┌Bus · current tab");
        let screen = p.screen_contents();
        let right_corner = screen
            .lines()
            .nth(menu_y as usize)
            .unwrap()
            .chars()
            .skip(menu_x as usize)
            .position(|ch| ch == '┐')
            .unwrap() as u16
            + menu_x;
        assert!(menu_x > 0);
        assert_eq!(menu_y, trigger_y + 1);
        assert_eq!(right_corner, trigger_x + 3);
    };
    let bus = testbus::bus_owned();
    let mut p = spawn(&bus.address, 80, 24, None);
    services(&mut p);
    let (x, y) = location(&p, "Bus▾");
    assert_eq!(y, 1);
    assert!(!p.screen_contents().lines().next().unwrap().contains("Bus▾"));
    p.mouse_click(x, y, MouseButton::Left).unwrap();
    wait(&mut p, "Bus · current tab");
    assert_menu_position(&p);
    assert!(p.contains("Name"));
    assert!(p.contains("org.busx.ScrollA"));
    assert!(!p.contains("[ Connect ]"));
    // Clicking the service list closes the menu without opening a service.
    p.mouse_click(5, 3, MouseButton::Left).unwrap();
    p.wait_for(|s| !s.contains("Bus · current tab")).unwrap();
    assert!(p.contains("Services ("));
    // The trigger toggles the menu, and browser pages have no bus trigger.
    let (x, y) = location(&p, "Bus▾");
    p.mouse_click(x, y, MouseButton::Left).unwrap();
    wait(&mut p, "Bus · current tab");
    p.mouse_click(x, y, MouseButton::Left).unwrap();
    p.wait_for(|s| !s.contains("Bus · current tab")).unwrap();
    open_service(&mut p, SERVICE);
    assert!(!p.contains("Bus▾"));
    text(&mut p, "b");
    p.wait_for(|s| !s.contains("Bus · current tab")).unwrap();
    key(&mut p, KeyCode::Esc);
    services(&mut p);
    let (x, y) = location(&p, "Bus▾");
    p.mouse_click(x, y, MouseButton::Left).unwrap();
    wait(&mut p, "Bus · current tab");
    p.resize(32, 10).unwrap();
    p.wait_for(|s| s.lines().count() == 10 && s.lines().all(|line| line.chars().count() <= 32))
        .unwrap();
    assert_menu_position(&p);
    p.send_key_with_mods(KeyCode::Char('u'), KeyModifiers::CTRL)
        .unwrap();
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "Enter a D-Bus address");
    assert!(!p.contains("[ Connect ]"));
    assert!(p.contains("Enter · Esc"));
    snapshot("bus_dropdown_error_32x10", &p);
    let (x, y) = location(&p, "Bus▾");
    p.mouse_click(x + 3, y, MouseButton::Left).unwrap();
    p.wait_for(|s| !s.contains("Bus · current tab")).unwrap();
}

fn start_property_watch(p: &mut TuiProbe) {
    start_property_watch_for(p, SERVICE);
}
fn start_property_watch_for(p: &mut TuiProbe, service: &str) {
    open_interface_for(p, service);
    probe_property(p, "volume");
    key(p, KeyCode::Enter);
    text(p, "ll");
    key(p, KeyCode::Enter);
    wait(p, "Waiting for events...");
}
fn match_rules(conn: &Connection) -> HashMap<String, Vec<String>> {
    Proxy::new(
        conn,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus.Debug.Stats",
    )
    .unwrap()
    .call("GetAllMatchRules", &())
    .unwrap()
}
fn property_subscriptions(conn: &Connection) -> usize {
    match_rules(conn)
        .values()
        .flatten()
        .filter(|rule| rule.contains("PropertiesChanged") && rule.contains(SERVICE))
        .count()
}

#[test]
fn property_watch_runs_in_place_collects_in_background_and_cancels_on_close() {
    let bus = testbus::bus_owned();
    let conn = connection(&bus.address);
    let proxy = target(&conn);
    let baseline = property_subscriptions(&conn);
    let mut p = spawn(&bus.address, 80, 24, None);
    start_property_watch(&mut p);
    poll(|| property_subscriptions(&conn) == baseline + 1);
    assert!(!p.screen_contents().lines().next().unwrap().contains("2 "));
    snapshot("watch_80x24", &p);
    text(&mut p, "t");
    services(&mut p);
    proxy.set_property("volume", 3.25f64).unwrap();
    text(&mut p, "gt");
    wait(&mut p, "3.25");
    wait(&mut p, "1 events");
    text(&mut p, "q");
    wait(&mut p, "Services (");
    poll(|| property_subscriptions(&conn) == baseline);
    assert!(p.is_running());
    close_last(&mut p);
}

#[test]
fn watch_pause_clear_stop_and_escape_restore_browser() {
    let bus = testbus::bus_owned();
    let conn = connection(&bus.address);
    let proxy = target(&conn);
    let baseline = property_subscriptions(&conn);
    let mut p = spawn(&bus.address, 80, 24, None);
    start_property_watch(&mut p);
    text(&mut p, "p");
    wait(&mut p, "PAUSED DISPLAY");
    proxy.set_property("volume", 4.25f64).unwrap();
    wait(&mut p, "1 events");
    assert!(!p.contains("4.25"));
    text(&mut p, "p");
    wait(&mut p, "4.25");
    text(&mut p, "x");
    wait(&mut p, "retained 0/1000");
    text(&mut p, "s");
    wait(&mut p, "stopped");
    poll(|| property_subscriptions(&conn) == baseline);
    key(&mut p, KeyCode::Esc);
    wait(&mut p, "Enter run");
    assert!(p.contains("volume"));
    // Restart, then Esc must cancel an active subscription too.
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "Waiting for events...");
    poll(|| property_subscriptions(&conn) == baseline + 1);
    key(&mut p, KeyCode::Esc);
    wait(&mut p, "Enter run");
    poll(|| property_subscriptions(&conn) == baseline);
    // Opening a watch result in the editor keeps the subscription active.
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "Waiting for events...");
    poll(|| property_subscriptions(&conn) == baseline + 1);
    proxy.set_property("volume", 6.75f64).unwrap();
    wait(&mut p, "6.75");
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "Opened in editor");
    poll(|| property_subscriptions(&conn) == baseline + 1);
    proxy.set_property("volume", 7.25f64).unwrap();
    wait(&mut p, "7.25");
    wait(&mut p, "2 events");
    click_up(&mut p, 1);
    wait(&mut p, "Enter run");
    poll(|| property_subscriptions(&conn) == baseline);
}

#[test]
fn method_watch_receives_actual_call() {
    let bus = testbus::bus_owned();
    let conn = connection(&bus.address);
    let proxy = target(&conn);
    let mut p = spawn(&bus.address, 80, 24, None);
    open_interface(&mut p);
    method(&mut p, "BumpVolume");
    text(&mut p, "l");
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "Waiting for events...");
    let _: f64 = proxy.call("BumpVolume", &()).unwrap();
    wait(&mut p, "1 events");
    wait(&mut p, "BumpVolume");
    key(&mut p, KeyCode::Esc);
    wait(&mut p, "Enter run");
}

fn copy_tool(p: &mut TuiProbe, clip: &ClipboardMock, tool: &str) -> String {
    text(p, "c");
    wait(p, "Copy command");
    // Tool choices are horizontal and include disabled entries.
    let index = match tool {
        "dbus-send" => 0,
        "busctl" => 1,
        "busx" => 4,
        _ => unreachable!(),
    };
    for _ in 0..index {
        key(p, KeyCode::Right);
    }
    key(p, KeyCode::Enter);
    wait(p, "Copied");
    poll(|| !clip.contents().is_empty());
    clip.contents()
}
fn copy_snapshot(name: &str, command: &str, address: &str) {
    insta::assert_snapshot!(name, command.replace(address, "<SOCKET>"));
}

#[test]
fn copy_commands_match_selected_operation_and_quote_arguments() {
    let bus = testbus::bus_owned();
    let clip = ClipboardMock::new();
    let mut p = spawn(&bus.address, 100, 28, Some(&clip));
    open_interface(&mut p);
    probe_property(&mut p, "volume");
    edit(&mut p, "1.5");
    let command = copy_tool(&mut p, &clip, "dbus-send");
    copy_snapshot("copy_write_dbus_send", &command, &bus.address);
    key(&mut p, KeyCode::Esc);
    p.wait_for(|s| !s.contains("Copy command · Esc close"))
        .unwrap();
    // Read executes explicitly and its copy describes Get, not the draft Write.
    key(&mut p, KeyCode::Enter);
    text(&mut p, "l");
    call(&mut p);
    text(&mut p, "j");
    wait(&mut p, "Enter open in editor");
    fs::write(&clip.log_path, "").unwrap();
    let command = copy_tool(&mut p, &clip, "dbus-send");
    copy_snapshot("copy_read_dbus_send", &command, &bus.address);
    key(&mut p, KeyCode::Esc);
    p.wait_for(|s| !s.contains("Copy command · Esc close"))
        .unwrap();
    key(&mut p, KeyCode::Esc);
    wait(&mut p, "h/l fold");
    key(&mut p, KeyCode::Tab); // navigation -> members
    method(&mut p, "Join");
    edit(&mut p, "1 $foo");
    fs::write(&clip.log_path, "").unwrap();
    let command = copy_tool(&mut p, &clip, "busx");
    assert!(command.contains("'$foo'"), "{command}");
    assert!(!command.contains("\"$foo\""));
    copy_snapshot("copy_join_busx", &command, &bus.address);
}

#[test]
fn copy_command_rejects_unparseable_arguments() {
    let bus = testbus::bus_owned();
    let clip = ClipboardMock::new();
    let mut p = spawn(&bus.address, 100, 28, Some(&clip));
    open_interface(&mut p);
    method(&mut p, "EchoBool");
    edit(&mut p, "maybe");

    // Copy uses the same busctl-style parser as execution. Invalid input is
    // shown in the result area instead of opening a popup with a bad command.
    text(&mut p, "c");
    wait(&mut p, "invalid boolean `maybe`");
    assert!(!p.contains("Copy command · Esc close"));
    assert!(clip.contents().is_empty());
}

#[test]
fn copy_container_call_and_watch_commands() {
    let bus = testbus::bus_owned();
    let clip = ClipboardMock::new();
    let mut p = spawn(&bus.address, 100, 28, Some(&clip));
    open_interface(&mut p);
    method(&mut p, "TakeHints");
    wait(&mut p, "Input syntax: Esc, then ? for help");
    text(&mut p, "?");
    key(&mut p, KeyCode::PageDown);
    key(&mut p, KeyCode::PageDown);
    wait(&mut p, "Input syntax follows busctl (see man busctl).");
    wait(
        &mut p,
        "Example: as takes 2 hello world; empty arrays take 0.",
    );
    key(&mut p, KeyCode::Esc);
    wait(&mut p, "Input syntax: Esc, then ? for help");
    edit(&mut p, "0");
    let command = copy_tool(&mut p, &clip, "busctl");
    // This scenario has no quoted tokens: execute the copied argv directly.
    let mut words = command.split_whitespace();
    let output = std::process::Command::new(words.next().unwrap())
        .args(words)
        .output()
        .expect("run copied busctl command");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "u 0");
    copy_snapshot("copy_container_busctl", &command, &bus.address);
    key(&mut p, KeyCode::Esc);
    p.wait_for(|s| !s.contains("Copy command · Esc close"))
        .unwrap();
    key(&mut p, KeyCode::Enter);
    text(&mut p, "l");
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "Waiting for events...");
    fs::write(&clip.log_path, "").unwrap();
    let command = copy_tool(&mut p, &clip, "busctl");
    copy_snapshot("copy_watch_busctl", &command, &bus.address);
}

// Extra D-Bus objects belong to these scenarios only, keeping CLI fixtures stable.
struct UiFixture {
    started: flume::Sender<()>,
    release: flume::Receiver<()>,
}

#[zbus::interface(name = "org.busx.UiFixture")]
impl UiFixture {
    #[zbus(property)]
    fn ready(&self) -> bool {
        true
    }

    #[zbus(signal)]
    async fn changed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        value: &str,
    ) -> zbus::Result<()>;

    async fn delayed_echo(&self, short: String, much_longer_parameter_name: String) -> String {
        self.started.send_async(()).await.unwrap();
        self.release.recv_async().await.unwrap();
        format!("{short}/{much_longer_parameter_name}")
    }
}

struct ForeignInterface;
#[zbus::interface(name = "org.example.UnrelatedNamespace")]
impl ForeignInterface {
    #[zbus(property)]
    fn label(&self) -> &str {
        "foreign-interface"
    }
}

fn ui_fixture(address: &str) -> (Connection, flume::Receiver<()>, flume::Sender<()>) {
    let (started_tx, started_rx) = flume::bounded(2);
    let (release_tx, release_rx) = flume::bounded(2);
    let mut builder = Builder::address(address)
        .unwrap()
        .name("org.busx.UiFixture")
        .unwrap()
        .serve_at(
            "/org/busx/Ui",
            UiFixture {
                started: started_tx,
                release: release_rx,
            },
        )
        .unwrap()
        .serve_at("/org/busx/Ui", ForeignInterface)
        .unwrap();
    for index in 0..12 {
        builder = builder
            .serve_at(format!("/org/busx/Ui/child_{index}"), ForeignInterface)
            .unwrap();
    }
    (builder.build().unwrap(), started_rx, release_tx)
}

fn open_ui_interface(p: &mut TuiProbe) {
    open_service(p, "org.busx.UiFixture");
    filter(p, "org.busx.UiFixture");
    key(p, KeyCode::Down);
    key(p, KeyCode::Enter);
    p.wait_for(|s| s.contains("Ready") && s.contains("Methods") && !s.contains("loading"))
        .unwrap();
}

#[test]
fn mouse_filter_cursor_stays_in_services_navigation_and_members() {
    let bus = testbus::bus_owned();
    let (_fixture, _started, _release) = ui_fixture(&bus.address);
    let mut p = spawn(&bus.address, 80, 24, None);
    let insert_in_filter = |p: &mut TuiProbe, query: &str, offset: usize| {
        filter(p, query);
        let (x, y) = location(p, "Filter /");
        p.mouse_click(x + 8 + offset as u16, y, MouseButton::Left)
            .unwrap();
        text(p, "X");
        wait(
            p,
            &format!("Filter /{}X{}", &query[..offset], &query[offset..]),
        );
        key(p, KeyCode::Backspace);
        wait(p, &format!("Filter /{query}"));
        key(p, KeyCode::Esc);
        p.wait_for(|s| !s.contains("Filter /")).unwrap();
    };
    services(&mut p);
    insert_in_filter(&mut p, "ScrollA", 2);
    open_service(&mut p, "org.busx.UiFixture");
    insert_in_filter(&mut p, "org.busx.UiFixture", 9);
    filter(&mut p, "org.busx.UiFixture");
    key(&mut p, KeyCode::Down);
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "Ready");
    let (x, y) = location(&p, "Methods");
    p.mouse_click(x, y, MouseButton::Left).unwrap();
    wait(&mut p, "DelayedEcho");
    insert_in_filter(&mut p, "DelayedEcho", 7);
    assert!(p.contains("Enter details"));
}

#[test]
fn mouse_up_from_browser_ignores_form_focus_and_preserves_drafts() {
    let bus = testbus::bus_owned();
    let (_fixture, started, _release) = ui_fixture(&bus.address);
    let mut p = spawn(&bus.address, 80, 24, None);
    services(&mut p);
    assert!(!p.contains(" ^ "));
    open_ui_interface(&mut p);
    method(&mut p, "DelayedEcho");
    text(&mut p, "i");
    text(&mut p, "draft");
    wait(&mut p, "draft");
    wait(&mut p, "INSERT");
    // All three columns, including both spaces, are clickable. Returning
    // from an input or action goes straight to Services in the wide layout.
    click_up(&mut p, 0);
    services(&mut p);
    assert!(!p.contains(" ^ "));
    open_service(&mut p, "org.busx.UiFixture");
    wait(&mut p, "NORMAL");
    assert!(p.contains("draft"));
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "Enter run");
    click_up(&mut p, 1);
    services(&mut p);
    open_service(&mut p, "org.busx.UiFixture");
    assert!(p.contains("draft"));
    text(&mut p, "n");
    filter(&mut p, "org.busx.UiFixture");
    click_up(&mut p, 2);
    services(&mut p);
    assert!(!p.contains("Filter /"));
    assert!(!p.contains(" ^ "));
    assert!(started.try_recv().is_err());
    assert!(p.is_running());
}

#[test]
fn mouse_narrow_navigation_opens_members_and_preserves_parameter_drafts() {
    let bus = testbus::bus_owned();
    let (_fixture, started, _release) = ui_fixture(&bus.address);
    let mut p = spawn(&bus.address, 64, 24, None);
    open_service(&mut p, "org.busx.UiFixture");
    filter(&mut p, "org.busx.UiFixture");
    let (x, y) = location(&p, "@ org.busx.UiFixture");
    p.mouse_click(x, y, MouseButton::Left).unwrap();
    wait(&mut p, "Ready");
    assert!(!p.contains("Navigation"));
    assert!(!p.contains("Filter /"));
    let (x, y) = location(&p, "Methods");
    p.mouse_click(x, y, MouseButton::Left).unwrap();
    wait(&mut p, "DelayedEcho");
    let (x, y) = location(&p, "DelayedEcho");
    for _ in 0..3 {
        p.mouse_click(x, y, MouseButton::Left).unwrap();
    }
    wait(&mut p, "Enter details");
    let (_, y) = location(&p, "short  s");
    p.mouse_click(36, y, MouseButton::Left).unwrap();
    text(&mut p, "alpha");
    wait(&mut p, "alpha");
    let (_, y) = location(&p, "much_longer_parameter_name");
    p.mouse_click(36, y, MouseButton::Left).unwrap();
    text(&mut p, "beta");
    wait(&mut p, "beta");
    let (x, y) = location(&p, "alpha");
    p.mouse_click(x + 2, y, MouseButton::Left).unwrap();
    text(&mut p, "X");
    wait(&mut p, "alXpha");
    assert!(p.contains("beta"));
    click_up(&mut p, 0);
    wait(&mut p, "Navigation");
    filter(&mut p, "org.busx.UiFixture");
    let (x, y) = location(&p, "@ org.busx.UiFixture");
    p.mouse_click(x, y, MouseButton::Left).unwrap();
    wait(&mut p, "Enter details");
    assert!(p.contains("alXpha"));
    assert!(p.contains("beta"));
    assert!(started.try_recv().is_err());
    p.resize(32, 12).unwrap();
    p.wait_for(|s| s.lines().count() == 12 && s.lines().all(|line| line.chars().count() <= 32))
        .unwrap();
    click_up(&mut p, 2);
    wait(&mut p, "Navigation");
    click_up(&mut p, 1);
    services(&mut p);
    assert!(!p.contains(" ^ "));
}

#[test]
fn mouse_selection_inputs_and_wheel_keep_focus_at_the_pointer_target() {
    let bus = testbus::bus_owned();
    let (_fixture, started, _release) = ui_fixture(&bus.address);
    let mut p = spawn(&bus.address, 80, 24, None);
    open_ui_interface(&mut p);
    text(&mut p, "l");
    wait(&mut p, "DelayedEcho");
    let (x, y) = location(&p, "DelayedEcho");
    for _ in 0..3 {
        p.mouse_click(x, y, MouseButton::Left).unwrap();
    }
    // Filtering proves repeated clicks kept keyboard focus in the member list.
    filter(&mut p, "DelayedEcho");
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "NORMAL");
    edit(&mut p, "alpha");
    key(&mut p, KeyCode::Down);
    edit(&mut p, "beta");
    let (x, y) = location(&p, "alpha");
    p.mouse_click(x + 2, y, MouseButton::Left).unwrap();
    text(&mut p, "X");
    wait(&mut p, "alXpha");
    wait(&mut p, "INSERT");
    let (x, y) = location(&p, "beta");
    p.mouse_click(x + 2, y, MouseButton::Left).unwrap();
    text(&mut p, "Y");
    wait(&mut p, "beYta");
    p.resize(80, 20).unwrap();
    p.wait_for(|s| s.lines().count() == 20 && s.contains("beYta"))
        .unwrap();
    let first_nav = p.screen_contents_crop(0, 3, 25, 1);
    p.mouse_scroll(8, 5, ScrollDirection::Down).unwrap();
    p.wait_for(|s| {
        s.lines()
            .nth(3)
            .map(|line| line.chars().take(25).collect::<String>())
            != Some(first_nav.clone())
    })
    .unwrap();
    // Scrolling another pane must preserve the active field and insertion mode.
    text(&mut p, "Z");
    wait(&mut p, "beYZta");
    p.mouse_scroll(35, 6, ScrollDirection::Down).unwrap();
    text(&mut p, "!");
    wait(&mut p, "beYZ!ta");
    assert!(p.contains("alXpha"));
    assert!(started.try_recv().is_err());
    let (x, y) = location(&p, "@ org.busx.U");
    p.mouse_click(x, y, MouseButton::Left).unwrap();
    p.mouse_click(x, y, MouseButton::Left).unwrap();
    wait(&mut p, "h/l fold");
    assert!(p.contains("alXpha"));
    assert!(p.contains("beYZ!ta"));
    let header = p.screen_contents().lines().next().unwrap().to_string();
    assert!(
        header.contains("🔍") && header.contains("…UiFixture"),
        "{header}"
    );
    assert!(!header.contains("/Ui"), "{header}");
}

#[test]
fn switching_buses_discards_replies_from_the_previous_tab_identity() {
    let first = testbus::bus_owned();
    let second = testbus::bus_owned();
    let (_first_fixture, first_started, first_release) = ui_fixture(&first.address);
    let (_second_fixture, second_started, second_release) = ui_fixture(&second.address);
    let mut p = spawn(&first.address, 80, 24, None);
    open_ui_interface(&mut p);
    method(&mut p, "DelayedEcho");
    edit(&mut p, "old");
    key(&mut p, KeyCode::Down);
    edit(&mut p, "bus");
    key(&mut p, KeyCode::Down);
    key(&mut p, KeyCode::Enter);
    first_started.recv_timeout(Duration::from_secs(5)).unwrap();
    text(&mut p, "n");
    key(&mut p, KeyCode::Esc);
    services(&mut p);
    custom_bus(&mut p, &second.address);
    services(&mut p);
    open_ui_interface(&mut p);
    method(&mut p, "DelayedEcho");
    edit(&mut p, "new");
    key(&mut p, KeyCode::Down);
    edit(&mut p, "bus");
    key(&mut p, KeyCode::Down);
    key(&mut p, KeyCode::Enter);
    second_started.recv_timeout(Duration::from_secs(5)).unwrap();
    first_release.send(()).unwrap();
    second_release.send(()).unwrap();
    wait(&mut p, "new/bus");
    assert!(p.contains("SUCCESS"));
    assert!(!p.contains("old/bus"));
}

#[test]
fn mouse_command_tools_select_without_copying_until_copy_is_clicked() {
    let bus = testbus::bus_owned();
    let clip = ClipboardMock::new();
    let mut p = spawn(&bus.address, 100, 28, Some(&clip));
    open_interface(&mut p);
    probe_property(&mut p, "volume");
    text(&mut p, "c");
    wait(&mut p, "Copy command · Esc close");
    let (x, y) = location(&p, "dbus-send");
    p.mouse_click(x, y, MouseButton::Left).unwrap();
    p.mouse_click(x, y, MouseButton::Left).unwrap();
    let (x, y) = location(&p, "busctl");
    p.mouse_click(x, y, MouseButton::Left).unwrap();
    wait(&mut p, "busctl --address=");
    let (x, y) = location(&p, "[ Copy ]");
    p.mouse_click(x + 2, y, MouseButton::Left).unwrap();
    wait(&mut p, "Copied");
    poll(|| !clip.contents().is_empty());
    let copied = clip.contents();
    assert!(copied.starts_with("busctl "), "{copied}");
    assert_eq!(copied.matches("busctl ").count(), 1, "{copied}");
    assert!(!copied.contains("dbus-send"), "{copied}");
}

#[test]
fn aligned_parameters_and_background_reply_stay_with_the_owner_tab() {
    let bus = testbus::bus_owned();
    let (_fixture, started, release) = ui_fixture(&bus.address);
    let mut p = spawn(&bus.address, 80, 24, None);
    open_ui_interface(&mut p);
    method(&mut p, "DelayedEcho");
    edit(&mut p, "alpha");
    key(&mut p, KeyCode::Down);
    edit(&mut p, "beta");
    let screen = p.screen_contents();
    let alpha_col = screen
        .lines()
        .find_map(|line| {
            line.find("alpha")
                .map(|offset| line[..offset].chars().count())
        })
        .unwrap();
    let beta_col = screen
        .lines()
        .find_map(|line| {
            line.find("beta")
                .map(|offset| line[..offset].chars().count())
        })
        .unwrap();
    assert_eq!(
        alpha_col, beta_col,
        "parameter input columns must align\n{screen}"
    );
    assert!(
        started.try_recv().is_err(),
        "editing must not invoke the method"
    );
    snapshot("aligned_parameters_80x24", &p);
    key(&mut p, KeyCode::Down);
    key(&mut p, KeyCode::Enter);
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    text(&mut p, "t");
    services(&mut p);
    release.send(()).unwrap();
    // While this tab processes input, the previous tab's reply may arrive.
    filter(&mut p, "ScrollB");
    text(&mut p, "q");
    wait(&mut p, "Filter /ScrollBq"); // q is literal in filter
    assert!(!p.contains("SUCCESS"));
    key(&mut p, KeyCode::Esc);
    p.wait_for(|s| !s.contains("Filter /")).unwrap();
    text(&mut p, "gt");
    wait(&mut p, "alpha/beta");
    assert!(p.contains("SUCCESS"));
    // A late reply to a closed tab must also leave its neighbour intact.
    key(&mut p, KeyCode::Enter);
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    text(&mut p, "q");
    wait(&mut p, "Services (");
    release.send(()).unwrap();
    filter(&mut p, "ScrollD");
    wait(&mut p, "org.busx.ScrollD");
    assert!(!p.contains("alpha/beta"));
}

#[test]
fn many_objects_and_unrelated_interface_names_remain_distinguishable() {
    let bus = testbus::bus_owned();
    let (_fixture, _started, _release) = ui_fixture(&bus.address);
    let mut p = spawn(&bus.address, 80, 24, None);
    open_ui_interface(&mut p);
    text(&mut p, "n");
    filter(&mut p, "org.example.UnrelatedNamespace");
    for _ in 0..32 {
        key(&mut p, KeyCode::Up);
    }
    key(&mut p, KeyCode::Down);
    p.wait_for(|s| s.lines().nth(1) == Some(" ^ org.example.UnrelatedNamespace"))
        .unwrap();
    snapshot("many_objects_and_interfaces_80x24", &p);
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "foreign-interface");
    assert!(p.contains("org.example.UnrelatedNamespace"));
}

#[test]
fn signal_watch_filters_sender_path_and_member() {
    let bus = testbus::bus_owned();
    let (fixture, _started, _release) = ui_fixture(&bus.address);
    let other = connection(&bus.address);
    let mut p = spawn(&bus.address, 80, 24, None);
    open_ui_interface(&mut p);
    text(&mut p, "ll");
    member(&mut p, "Changed");
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "Waiting for events...");
    let emit = |conn: &Connection, path: &str, member: &str, value: &str| {
        conn.emit_signal(None::<&str>, path, "org.busx.UiFixture", member, &(value,))
            .unwrap();
    };
    emit(&other, "/org/busx/Ui", "Changed", "wrong-sender");
    emit(&fixture, "/org/busx/Ui/child_0", "Changed", "wrong-path");
    emit(&fixture, "/org/busx/Ui", "OtherSignal", "wrong-member");
    emit(&fixture, "/org/busx/Ui", "Changed", "expected-signal");
    wait(&mut p, "expected-signal");
    assert!(p.contains("1 events"));
    assert!(!p.contains("wrong-"));
}

struct OtherMethodInterface;
#[zbus::interface(name = "org.busx.Test")]
impl OtherMethodInterface {
    fn bump_volume(&self) -> f64 {
        -1.0
    }
}

#[test]
fn method_watch_ignores_calls_to_another_service_with_the_same_interface() {
    let bus = testbus::bus_owned();
    let other = Builder::address(bus.address.as_str())
        .unwrap()
        .name("org.busx.Decoy")
        .unwrap()
        .serve_at(PATH, OtherMethodInterface)
        .unwrap()
        .build()
        .unwrap();
    let conn = connection(&bus.address);
    let mut p = spawn(&bus.address, 80, 24, None);
    open_interface(&mut p);
    method(&mut p, "BumpVolume");
    text(&mut p, "l");
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "Waiting for events...");
    let decoy = Proxy::new(&conn, "org.busx.Decoy", PATH, SERVICE).unwrap();
    let _: f64 = decoy.call("BumpVolume", &()).unwrap();
    let _: f64 = target(&conn).call("BumpVolume", &()).unwrap();
    wait(&mut p, "1 events");
    assert!(!p.contains("2 events"));
    drop(other);
}
