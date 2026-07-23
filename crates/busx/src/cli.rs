// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use clap::{Parser, Subcommand};
use clap_complete::ArgValueCompleter;

use crate::complete;

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
    /// that every object implements. The TUI hides them by default since
    /// they're rarely useful when browsing; this brings them back.
    #[arg(
        long,
        help = "Show standard D-Bus interfaces in the TUI (hidden by default)"
    )]
    pub show_standard_interfaces: bool,
    /// Emit machine-readable type-tagged JSON instead of human-friendly text.
    /// Global: applies to every subcommand. For `monitor` the JSON form is NDJSON
    /// (one object per message).
    #[arg(
        long,
        global = true,
        help = "Emit type-tagged JSON (default: human text)"
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
        #[arg(long, help = "List activatable (auto-startable) services instead of current ones")]
        activatable: bool,
    },
    /// Show interfaces/methods/signals/properties of an object.
    Introspect {
        #[arg(add = ArgValueCompleter::new(complete::complete_service))]
        service: String,
        #[arg(add = ArgValueCompleter::new(complete::complete_path))]
        object: String,
        #[arg(add = ArgValueCompleter::new(complete::complete_interface))]
        interface: Option<String>,
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
    /// Monitor bus messages. Defaults to a signal subscription (no privileges).
    /// Pass --all to see method calls/returns/errors too (BecomeMonitor; may be
    /// refused by the bus, in which case the command errors out).
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
        /// See method calls/returns/errors too (BecomeMonitor; privileged).
        #[arg(long, help = "Also capture method calls/returns/errors (BecomeMonitor)")]
        all: bool,
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
