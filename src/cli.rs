use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "busx", version, about = "D-Bus CLI (dbus-send/busctl/qdbus replacement)")]
pub struct Cli {
    #[arg(long, help = "Connect to the session bus; if that fails, fall back to the system bus (default)")]
    pub user: bool,
    #[arg(long, help = "Connect to the system bus")]
    pub system: bool,
    #[arg(long, value_name = "ADDRESS", help = "Connect to the bus at ADDRESS (e.g. unix:path=...)")]
    pub address: Option<String>,
    #[arg(long, help = "Verbose diagnostics on stderr")]
    pub verbose: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// List service names on the bus.
    List { #[arg(long)] unique: bool, #[arg(long)] acquired: bool, #[arg(long)] activatable: bool },
    /// Show the object path tree of services.
    Tree { services: Vec<String> },
    /// Show interfaces/methods/signals/properties of an object.
    Introspect { service: String, object: String, interface: Option<String> },
    /// Call a method.
    Call { service: String, object: String, interface: String, method: String, args: Vec<String> },
    /// Get properties (no property names => GetAll).
    Get { service: String, object: String, interface: Option<String>, props: Vec<String> },
    /// Set a property.
    Set { service: String, object: String, interface: String, property: String, signature: String, value: Vec<String> },
    /// Monitor bus messages.
    Monitor {
        services: Vec<String>,
        #[arg(long)] interface: Option<String>,
        #[arg(long)] member: Option<String>,
        #[arg(long)] path: Option<String>,
        #[arg(long)] sender: Option<String>,
        #[arg(long, value_name = "MATCH")] r#match: Option<String>,
        #[arg(long)] signals: bool,
        #[arg(long, value_name = "N")] limit_messages: Option<u64>,
        #[arg(long, value_name = "DUR")] timeout: Option<String>,
    },
    /// Generate shell completion script.
    Completion { shell: clap_complete::Shell },
    /// (hidden) dynamic completion candidate generator.
    #[command(name = "__complete", hide = true)]
    Complete { args: Vec<String> },
}
