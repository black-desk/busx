// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use clap::{Parser, Subcommand};

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
        #[arg(long)]
        unique: bool,
        #[arg(long)]
        acquired: bool,
        #[arg(long)]
        activatable: bool,
    },
    /// Show interfaces/methods/signals/properties of an object.
    Introspect {
        service: String,
        object: String,
        interface: Option<String>,
    },
    /// Call a method.
    Call {
        service: String,
        object: String,
        interface: String,
        method: String,
        signature: String,
        args: Vec<String>,
    },
    /// Get properties (no property names => GetAll).
    Get {
        service: String,
        object: String,
        interface: Option<String>,
        props: Vec<String>,
    },
    /// Set a property.
    Set {
        service: String,
        object: String,
        interface: String,
        property: String,
        signature: String,
        value: Vec<String>,
    },
    /// Monitor bus messages.
    Monitor {
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
        #[arg(long)]
        signals: bool,
        #[arg(long, value_name = "N")]
        limit_messages: Option<u64>,
        #[arg(long, value_name = "DUR")]
        timeout: Option<String>,
    },
    /// Generate shell completion script.
    #[command(hide = true)]
    Completion { shell: clap_complete::Shell },
}
