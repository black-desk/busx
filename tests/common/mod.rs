use std::process::{Command, Stdio};
use std::sync::OnceLock;
use zbus::blocking::connection::Builder;
use zbus::blocking::Connection;
use zbus::interface;

/// Test service. Methods/properties are chosen to exercise every busx feature:
/// - `counts` (a{uu}): non-string-key dict — exercises spec §7.2 safety rule
/// - `hints` (a{sv}): string-key dict-of-variant
/// - `volume`: settable property; changing it emits PropertiesChanged (monitor test)
/// - `take_hints`/`join`: targets for encode (call) tests
pub struct TestIface {
    volume: f64,
}

#[interface(name = "org.busx.Test")]
impl TestIface {
    // Pin D-Bus property names to lowercase so the fixture matches its
    // documented contract (`volume`/`name`/`counts`/`hints`). zbus otherwise
    // exposes Rust snake_case getters as PascalCase.
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
}

pub struct TestBus {
    pub address: String,
    _conn: Connection,
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

static BUS: OnceLock<TestBus> = OnceLock::new();

/// Returns the shared test bus (started once per test binary).
pub fn bus() -> &'static TestBus {
    BUS.get_or_init(|| {
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

        TestBus {
            address,
            _conn: conn,
            daemon_pid,
        }
    })
}
