mod common;
use assert_cmd::Command;

/// Completing the 1st positional of `call` lists well-known service names,
/// including the test fixture's `org.busx.Test`.
#[test]
fn complete_service_position_lists_test_service() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args(["--address", &addr, "__complete", "call", ""])
        .ok()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("org.busx.Test"), "service candidates:\n{stdout}");
    // Unique (`:`-prefixed) names are filtered out.
    assert!(
        !stdout.lines().any(|l| l.starts_with(':')),
        "unique names should be filtered: {stdout}"
    );
}

/// Prefix matching: asking for services starting with `org.busx.` narrows
/// the candidates rather than listing the whole bus.
#[test]
fn complete_service_position_filters_by_prefix() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args(["--address", &addr, "__complete", "call", "org.busx."])
        .ok()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.lines().all(|l| l.starts_with("org.busx.")),
        "prefix filter should hold: {stdout}"
    );
    assert!(stdout.contains("org.busx.Test"));
}

/// Completing the 2nd positional (object path) of `introspect` introspects `/`
/// and offers the immediate child paths as full paths.
#[test]
fn complete_path_position_lists_child_paths() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args(["--address", &addr, "__complete", "introspect", "org.busx.Test", ""])
        .ok()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("/org"),
        "child path candidate expected:\n{stdout}"
    );
}

/// Completing the 3rd positional (interface) lists the object's interfaces,
/// including the fixture's `org.busx.Test`.
#[test]
fn complete_interface_position_lists_interfaces() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--address",
            &addr,
            "__complete",
            "introspect",
            "org.busx.Test",
            "/org/busx/Test",
            "",
        ])
        .ok()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("org.busx.Test"), "interface candidates:\n{stdout}");
    assert!(
        stdout.contains("org.freedesktop.DBus.Properties"),
        "standard interfaces expected: {stdout}"
    );
}

/// Completing the 4th positional of `call` (the method) lists the methods of
/// the chosen interface. The fixture exposes zbus's PascalCase method names.
#[test]
fn complete_method_position_lists_methods() {
    let addr = common::bus().address.clone();
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--address",
            &addr,
            "__complete",
            "call",
            "org.busx.Test",
            "/org/busx/Test",
            "org.busx.Test",
            "",
        ])
        .ok()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("BumpVolume"),
        "method candidates should include BumpVolume:\n{stdout}"
    );
}

/// Completion never fails the command, even when the bus is unreachable: an
/// invalid address yields no output and a successful exit.
#[test]
fn complete_silently_yields_nothing_on_bus_error() {
    let out = Command::cargo_bin("busx")
        .unwrap()
        .args([
            "--address",
            "unix:path=/nonexistent/busx-completion-test.sock",
            "__complete",
            "call",
            "",
        ])
        .ok()
        .unwrap();
    assert!(out.stdout.is_empty(), "no output on bus error");
    assert!(
        out.status.success(),
        "exit must be 0 on bus error (got {})",
        out.status
    );
}
