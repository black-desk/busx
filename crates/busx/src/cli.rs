// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use clap::{Parser, Subcommand, ValueEnum};
use clap_complete::ArgValueCompleter;

use crate::complete;
/// D-Bus message type to capture for `monitor`.
///
/// `signal` is special: it uses a plain, unprivileged signal subscription
/// (every bus accepts it). Any other type — or omitting `--type` entirely —
/// switches `monitor` into BecomeMonitor mode, which also sees method calls /
/// returns / errors but is privileged and may be refused by the bus.
#[derive(Clone, Copy, Debug, ValueEnum, PartialEq, Eq)]
pub enum MonitorType {
    #[value(name = "signal")]
    Signal,
    #[value(name = "method_call")]
    MethodCall,
    #[value(name = "method_return")]
    MethodReturn,
    #[value(name = "error")]
    Error,
}

#[derive(Parser, Debug)]
#[command(
    name = "busx",
    version,
    about = "D-Bus CLI (dbus-send/busctl/qdbus replacement)"
)]
pub struct Cli {
    #[arg(
        long,
        help = "Connect to the session bus (the default falls back to the system bus on failure)"
    )]
    pub user: bool,
    #[arg(long, help = "Connect to the system bus")]
    pub system: bool,
    #[arg(
        long,
        value_name = "ADDRESS",
        help = "Connect to the bus at ADDRESS (e.g. unix:path=...)"
    )]
    pub address: Option<String>,
    #[arg(
        short = 'v',
        action = clap::ArgAction::Count,
        global = true,
        help = "Increase log verbosity (-v / -vv / -vvv)"
    )]
    pub verbose: u8,
    #[arg(
        long = "log",
        value_name = "PATH",
        help = "TUI log file (default: $XDG_CACHE_HOME/busx/busx.log)"
    )]
    pub log: Option<String>,
    /// Show the standard D-Bus interfaces (Properties, Introspectable, Peer)
    /// that every object implements. They are hidden by default since they're
    /// rarely useful when browsing; this brings them back.
    #[arg(long, help = "Show standard D-Bus interfaces hidden by default")]
    pub show_standard_interfaces: bool,
    /// Emit JSON instead of human-friendly text. Global, but only the
    /// value-producing subcommands honour it: for `call` / `get` each D-Bus
    /// value is type-tagged as `{"type":..,"data":..}`; `list` / `tree` emit
    /// plain JSON; `monitor` is NDJSON. Ignored (with a warning) by `set` /
    /// `introspect` / `emit`, which produce no JSON output.
    #[arg(
        long,
        global = true,
        help = "Emit JSON (default: human text)",
        long_help = "Emit JSON instead of human-friendly text.\n\
            Only the value-producing subcommands honour it: for `call` / `get` \
            each D-Bus value is type-tagged as `{\"type\":..,\"data\":..}`; \
            `list` / `tree` emit plain JSON; `monitor` is NDJSON. \
            Ignored (with a warning) by `set` / `introspect` / `emit`, which \
            produce no JSON output."
    )]
    pub json: bool,
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// List service names on the bus.
    List {
        #[arg(long, help = "Hide unique (`:1.x`) connection names")]
        no_unique: bool,
        #[arg(
            long,
            help = "List activatable (auto-startable) services instead of current ones"
        )]
        activatable: bool,
    },
    /// Dump the raw Introspect() XML for an object, verbatim.
    Introspect {
        #[arg(add = ArgValueCompleter::new(complete::complete_service))]
        service: String,
        #[arg(add = ArgValueCompleter::new(complete::complete_path))]
        object: String,
    },
    /// Call a method.
    Call {
        #[arg(add = ArgValueCompleter::new(complete::complete_service))]
        service: String,
        #[arg(add = ArgValueCompleter::new(complete::complete_path))]
        object: String,
        #[arg(add = ArgValueCompleter::new(complete::complete_interface))]
        interface: String,
        #[arg(add = ArgValueCompleter::new(complete::complete_method))]
        method: String,
        #[arg(add = ArgValueCompleter::new(complete::complete_signature))]
        signature: String,
        args: Vec<String>,
    },
    /// Get properties (no property names => GetAll).
    Get {
        #[arg(add = ArgValueCompleter::new(complete::complete_service))]
        service: String,
        #[arg(add = ArgValueCompleter::new(complete::complete_path))]
        object: String,
        #[arg(add = ArgValueCompleter::new(complete::complete_interface))]
        interface: Option<String>,
        #[arg(add = ArgValueCompleter::new(complete::complete_property))]
        props: Vec<String>,
    },
    /// Set a property.
    Set {
        #[arg(add = ArgValueCompleter::new(complete::complete_service))]
        service: String,
        #[arg(add = ArgValueCompleter::new(complete::complete_path))]
        object: String,
        #[arg(add = ArgValueCompleter::new(complete::complete_interface))]
        interface: String,
        #[arg(add = ArgValueCompleter::new(complete::complete_property))]
        property: String,
        signature: String,
        value: Vec<String>,
    },
    /// Monitor bus messages.
    ///
    /// With no `--type`, `monitor` becomes a bus monitor (BecomeMonitor) and
    /// shows every message crossing the bus — method calls, returns, errors,
    /// and signals — the same as `busctl monitor`. BecomeMonitor is privileged
    /// and may be refused by the bus; when it is, the command errors out rather
    /// than silently degrading.
    ///
    /// `--type signal` instead uses a plain signal subscription, which every
    /// bus accepts with no privileges. Any other `--type` (or none at all)
    /// selects BecomeMonitor.
    ///
    /// Once the subscription is live on the bus, `monitor` prints a `ready`
    /// event to stdout before any messages, so scripts can wait for it instead
    /// of sleeping: with `--json` that's a line `{"event":"ready"}`;
    /// in human mode a `busx: monitoring` line. Filter it out to keep
    /// only messages (`jq 'select(.type)'` / `grep -v '^busx: monitoring'`).
    Monitor {
        #[arg(add = ArgValueCompleter::new(complete::complete_service))]
        services: Vec<String>,
        #[arg(long)]
        interface: Option<String>,
        #[arg(long)]
        member: Option<String>,
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        sender: Option<String>,
        #[arg(long, value_name = "MATCH")]
        r#match: Option<String>,
        /// Message type to capture. `signal` uses an unprivileged signal
        /// subscription; any other type (or omitting `--type`) uses
        /// BecomeMonitor so method calls/returns/errors are visible too.
        #[arg(long, value_name = "TYPE")]
        r#type: Option<MonitorType>,
        #[arg(long, value_name = "N")]
        limit_messages: Option<u64>,
        #[arg(long, value_name = "DUR")]
        timeout: Option<String>,
    },
    /// Recursively list an object-path tree of a service.
    Tree {
        #[arg(add = ArgValueCompleter::new(complete::complete_service))]
        service: String,
    },
    /// Emit a D-Bus signal (for testing listeners on a private bus).
    Emit {
        /// Send to DEST (unicast) instead of broadcasting to all listeners.
        #[arg(long, value_name = "DEST")]
        destination: Option<String>,
        object: String,
        interface: String,
        member: String,
        signature: String,
        args: Vec<String>,
    },
    /// Generate shell completion script.
    #[command(hide = true)]
    Completion { shell: clap_complete::Shell },
}
