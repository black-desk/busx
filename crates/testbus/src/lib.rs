// SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Test bus fixture.
//!
//! [`bus_owned`] starts a fresh, owned test bus: it spins up a private
//! `dbus-daemon` and registers the `org.busx.Test` interface against it
//! in-process. Drop the returned [`TestBus`] to SIGTERM the daemon.
//!
//! Each test takes its own bus (rather than sharing a process-wide
//! singleton) so that (a) every bus is reaped when its test ends — a
//! `static` singleton is never dropped and would leak its daemon — and
//! (b) callers see a deterministic `:1.x` table regardless of what other
//! tests are running concurrently.

use std::process::{Command, Stdio};

use zbus::blocking::connection::Builder;
use zbus::interface;

/// Test service. Methods/properties are chosen to exercise every busx
/// feature:
/// - `counts` (a{uu}): non-string-key dict — exercises spec §7.2 safety rule
/// - `hints` (a{sv}): string-key dict-of-variant
/// - `volume`: settable property; changing it emits PropertiesChanged
///   (monitor test)
/// - `take_hints`/`join`: targets for encode (call) tests
/// - `echo_bool`: round-trip target for `b` signature encode tests
pub struct TestIface {
    volume: f64,
}

#[interface(name = "org.busx.Test")]
impl TestIface {
    // Pin D-Bus property names to lowercase so the fixture matches its
    // documented contract (`volume`/`name`/`counts`/`hints`). zbus
    // otherwise exposes Rust snake_case getters as PascalCase.
    #[zbus(property, name = "volume")]
    fn volume(&self) -> f64 {
        self.volume
    }
    #[zbus(property, name = "volume")]
    fn set_volume(&mut self, v: f64) {
        self.volume = v;
    }

    #[zbus(property, name = "name")]
    fn name(&self) -> String {
        "busx-test".to_string()
    }

    /// Non-string-key dict property (a{uu}) — for the decode safety test.
    #[zbus(property, name = "counts")]
    fn counts(&self) -> std::collections::HashMap<u32, u32> {
        [(1u32, 10u32), (2, 20)].into_iter().collect()
    }

    /// String-key dict-of-variant property (a{sv}).
    #[zbus(property, name = "hints")]
    fn hints(&self) -> std::collections::HashMap<String, zvariant::Value<'static>> {
        [("urgency".to_string(), zvariant::Value::U8(1))]
            .into_iter()
            .collect()
    }

    /// Takes a{sv}, returns entry count — encode target.
    fn take_hints(&self, hints: std::collections::HashMap<String, zvariant::Value<'_>>) -> u32 {
        hints.len() as u32
    }

    /// Joins a string array — encode target.
    fn join(&self, parts: Vec<String>) -> String {
        parts.join("-")
    }

    /// Bumps volume to trigger PropertiesChanged — monitor target.
    fn bump_volume(&mut self) -> f64 {
        self.volume += 1.0;
        self.volume
    }

    /// Returns a read-only fd to `/dev/null` — deterministic target for the
    /// unix-fd render test (readlink is always `/dev/null`, fstat is a char
    /// device). `h` over the wire; the connection negotiates fd passing.
    fn make_fd(&self) -> zvariant::OwnedFd {
        let f = std::fs::File::open("/dev/null").expect("open /dev/null");
        zvariant::OwnedFd::from(std::os::fd::OwnedFd::from(f))
    }

    /// Returns the read end of a fresh pipe — exercises the `pipe:[ino]` /
    /// FIFO-kind render (inode is non-deterministic, asserted by prefix).
    fn make_pipe_fd(&self) -> zvariant::OwnedFd {
        use std::os::fd::{FromRawFd, OwnedFd};
        let mut fds = [0i32; 2];
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0, "pipe(2)");
        let read_end = unsafe { OwnedFd::from_raw_fd(fds[0]) };
        unsafe { libc::close(fds[1]) }; // release the write end in this process
        zvariant::OwnedFd::from(read_end)
    }

    /// Returns `ay` = "hello" — bytestring render test (all printable ASCII).
    fn make_bytes(&self) -> Vec<u8> {
        b"hello".to_vec()
    }

    /// Returns `ay` with non-printable bytes — exercises the `\xNN` escaping.
    fn make_raw_bytes(&self) -> Vec<u8> {
        vec![0x00, 0xab, b'c', 0xff]
    }

    /// Returns `s` with embedded control characters — exercises string
    /// escaping (`\n` / `\t` named, other controls as `\u{NN}`) so they
    /// don't break the line-based output.
    fn make_control_string(&self) -> String {
        "a\tb\nc\u{1}d".to_string()
    }

    /// Echoes the input bool — round-trip target for the `b` signature, so
    /// `busx call` encode tests can verify bool parsing (true/yes/on/1 vs
    /// True/garbage/2) end-to-end through a real D-Bus call.
    fn echo_bool(&self, b: bool) -> bool {
        b
    }

    /// Returns a long string — exercises the Result screen's horizontal
    /// `<`/`>` clipping + scroll in the TUI snapshot tests (the value is
    /// deliberately longer than the 80-column result area's inner width).
    fn long_string(&self) -> String {
        "the quick brown fox jumps over the lazy dog near the riverbank; a truly lovely afternoon"
            .to_string()
    }
}

pub struct TestBus {
    pub address: String,
    _conn: zbus::blocking::Connection,
    daemon_pid: i32,
}

impl Drop for TestBus {
    fn drop(&mut self) {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(self.daemon_pid),
            nix::sys::signal::Signal::SIGTERM,
        );
    }
}

/// Starts a fresh, owned test bus. Drop the returned `TestBus` to
/// SIGTERM the underlying daemon.
///
/// Use this from tests that need a deterministic `:1.x` naming table —
/// e.g. e2e TUI tests that list_names and snapshot the result, where a
/// deterministic `:1.0` (daemon) + `:1.1` (fixture) + `:1.2` (self)
/// naming matters regardless of concurrent tests.
pub fn bus_owned() -> TestBus {
    start_bus_with_fixture()
}

fn start_bus_with_fixture() -> TestBus {
    // --fork: the parent prints "address\npid\n" to stdout then exits;
    // the daemon detaches and runs with the given pid. No pipe to hold.
    let out = Command::new("dbus-daemon")
        .args(["--session", "--fork", "--print-address=1", "--print-pid=1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .expect("dbus-daemon not found; install dbus");
    let s = String::from_utf8(out.stdout).expect("dbus-daemon output utf8");
    let mut lines = s.lines();
    let address = lines.next().expect("address line").trim().to_string();
    let daemon_pid: i32 = lines
        .next()
        .expect("pid line")
        .trim()
        .parse()
        .expect("pid number");

    let conn = Builder::address(address.as_str())
        .expect("build test-bus connection")
        .serve_at("/org/busx/Test", TestIface { volume: 0.5 })
        .expect("register test object")
        .serve_at("/org/busx/Test/sub", TestIface { volume: 0.0 })
        .expect("register sub object")
        .name("org.busx.Test")
        .expect("request name")
        .build()
        .expect("connect test bus");

    // A second, deliberately overlong well-known name so `list` can exercise
    // its NAME-column truncation.
    conn.request_name(
        "org.busx.TestServiceNameThatIsIntentionallyVeryLongSoItExceedsTheNameColumnWidthLimitOfFiftyFour",
    )
    .expect("request long name");

    // Extra well-known names all owned by this connection so the TUI's
    // service list has enough rows to exercise viewport scrolling in
    // end-to-end snapshot tests. Letter-suffixed (rather than numbered)
    // so they contain no digits — that way the e2e insta filter can
    // blanket-match any remaining digit string as a PID without
    // clobbering the service names.
    for suffix in ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L'] {
        conn.request_name(format!("org.busx.Scroll{suffix}"))
            .expect("request scroll name");
    }

    TestBus {
        address,
        _conn: conn,
        daemon_pid,
    }
}
