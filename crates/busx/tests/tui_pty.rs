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
use std::time::{Duration, Instant};

use portable_pty::CommandBuilder;
use tuiprobe::{KeyCode, MouseButton, ScrollDirection, TuiProbe};
use zbus::blocking::{Connection, Proxy, connection::Builder};

const SERVICE: &str = "org.busx.Test";
const PATH: &str = "/org/busx/Test";

fn spawn(addr: &str, cols: u16, rows: u16, clip: Option<&ClipboardMock>) -> TuiProbe {
    let mut probe = TuiProbe::builder()
        .cols(cols)
        .rows(rows)
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    let mut cmd = CommandBuilder::new(assert_cmd::cargo::cargo_bin!("busx"));
    cmd.args(["--address", addr]);
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
        .wait_for(|s| s.contains("Services (") && s.contains("Name") && !s.contains("loading"))
        .unwrap();
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
    open_service(probe, SERVICE);
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
    if let Some(header) = lines
        .iter()
        .position(|line| line.contains("Name") && line.contains("PID"))
    {
        let pid_end = lines[header].chars().position(|ch| ch == 'P').unwrap() + 3;
        for line in lines
            .iter_mut()
            .skip(header + 1)
            .take_while(|line| !line.starts_with('└'))
        {
            let mut cells: Vec<char> = line.chars().collect();
            if cells.len() < pid_end + 15 || cells.get(3).is_none_or(|ch| ch.is_whitespace()) {
                continue;
            }
            let process: String = cells[pid_end + 2..pid_end + 15].iter().collect();
            cells[pid_end - 7..pid_end].copy_from_slice(&"  <PID>".chars().collect::<Vec<_>>());
            if process.trim_start().starts_with("tui_pty-") {
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
fn call_zero_arg_method_and_expand_result() {
    let bus = testbus::bus_owned();
    let mut p = spawn(&bus.address, 80, 24, None);
    open_interface(&mut p);
    method(&mut p, "MakeFd");
    call(&mut p);
    wait(&mut p, "/dev/null");
    snapshot("method_result_80x24", &p);
    text(&mut p, "j");
    wait(&mut p, "Enter expand");
    key(&mut p, KeyCode::Enter);
    wait(&mut p, "Result · y copy");
    key(&mut p, KeyCode::Esc);
    wait(&mut p, "SUCCESS");
    assert!(p.contains("Methods"));
}

#[test]
fn result_horizontal_scroll() {
    let bus = testbus::bus_owned();
    let mut p = spawn(&bus.address, 80, 24, None);
    open_interface(&mut p);
    method(&mut p, "LongString");
    call(&mut p);
    wait(&mut p, "the quick brown fox");
    text(&mut p, "j");
    wait(&mut p, "Enter expand");
    snapshot("long_result_clipped", &p);
    key(&mut p, KeyCode::Right);
    wait(&mut p, "quick brown fox");
    p.wait_for(|s| !s.contains("\"the quick brown fox"))
        .unwrap();
    snapshot("long_result_scrolled", &p);
    let scrolled_line = p
        .screen_contents()
        .lines()
        .find(|line| line.contains("quick brown fox"))
        .unwrap()
        .to_string();
    text(&mut p, "t");
    services(&mut p);
    text(&mut p, "gt");
    wait(&mut p, &scrolled_line);
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
    p.mouse_click(22, 0, MouseButton::Left).unwrap();
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
    wait(&mut p, "3 Services");
    text(&mut p, "q");
    wait(&mut p, "2 Services");
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
    wait(&mut p, "2 Services");
    // Close the first, inactive tab via its visible ×, keeping tab 2 active.
    let header = p.screen_contents().lines().next().unwrap().to_string();
    let x = header.chars().position(|ch| ch == '×').unwrap() as u16;
    p.mouse_click(x, 0, MouseButton::Left).unwrap();
    p.wait_for(|s| !s.lines().next().unwrap().contains("2 Services"))
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
    wait(&mut p, "10 Se");
    snapshot("tabs_overflow_64x12", &p);
    text(&mut p, "1gt");
    wait(&mut p, "1 Ser");
    text(&mut p, "gT");
    wait(&mut p, "10 Se");
}

fn start_property_watch(p: &mut TuiProbe) {
    open_interface(p);
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
    wait(&mut p, "Enter expand");
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
    p.wait_for(|s| s.lines().nth(1) == Some("org.example.UnrelatedNamespace"))
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
