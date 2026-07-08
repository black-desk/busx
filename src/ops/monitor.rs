// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx monitor` — stream bus messages as NDJSON, with match-rule filtering
//! (spec §10).
//!
//! Each message is rendered as one JSON object per line:
//!
//! ```jsonc
//! {"type":"signal","sender":":1.x","destination":":1.y","path":"/...",
//!  "interface":"...","member":"...","serial":47,"reply_serial":null,
//!  "error":null,"signature":"sa{sv}as","flags":[],"ts":1720000000.123,
//!  "args":[ <type-tagged values...> ]}
//! ```
//!
//! There are two delivery modes:
//!
//! * `--signals` (and the default "all messages" mode): a D-Bus match rule is
//!   built from the convenience flags and `--match`, then either registered via
//!   `MessageIterator::for_match_rule` (signal subscription) or handed to
//!   [`org.freedesktop.DBus.Monitoring.BecomeMonitor`].
//! * Default (no `--signals`): the connection is converted into a bus monitor
//!   via `BecomeMonitor`, so it sees every message crossing the bus
//!   (method_call / method_return / error / signal) — the same mechanism
//!   `busctl monitor` uses. `BecomeMonitor` is privileged and may be refused by
//!   some bus configurations; when that happens we fall back to a signal-only
//!   match rule so the command degrades gracefully instead of hard-failing.

use crate::conn::connect;
use crate::error::{Error, Result};
use serde_json::{Value as Json, json};
use std::io::{BufWriter, Write};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use zbus::MatchRule;
use zbus::blocking::MessageIterator;
use zbus::message::{Flags, Type};
use zvariant::Structure;

/// Decode the message-flags byte into the spec's lowercased names (e.g.
/// `"no_reply_expected"`). Unknown/unused bits are dropped.
fn flags_of(flags: enumflags2::BitFlags<Flags>) -> Vec<&'static str> {
    let mut out = Vec::new();
    if flags.contains(Flags::NoReplyExpected) {
        out.push("no_reply_expected");
    }
    if flags.contains(Flags::NoAutoStart) {
        out.push("no_auto_start");
    }
    if flags.contains(Flags::AllowInteractiveAuth) {
        out.push("allow_interactive_authorization");
    }
    out
}

/// Epoch seconds at receipt, with fractional precision (f64).
fn epoch_secs() -> f64 {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    dur.as_secs() as f64 + dur.subsec_nanos() as f64 / 1_000_000_000.0
}

/// Render a single received message as the spec §10 JSON object.
///
/// The body is decoded as a `Structure` (Task 5's trick: this accepts any body
/// signature and yields the positional values as fields) and each field is
/// rendered type-tagged via [`crate::value::decode::to_tagged`]. A body that
/// fails to deserialize (e.g. an empty method return) degrades to `args: []`.
fn msg_to_json(m: &zbus::Message) -> Json {
    let h = m.header();

    let ty = match m.message_type() {
        Type::MethodCall => "method_call",
        Type::MethodReturn => "method_return",
        Type::Error => "error",
        Type::Signal => "signal",
    };

    let args: Vec<Json> = m
        .body()
        .deserialize::<Structure>()
        .map(|s| s.fields().iter().map(crate::value::decode::to_tagged).collect())
        .unwrap_or_default();

    json!({
        "type": ty,
        "sender": h.sender().map(|s| s.to_string()),
        "destination": h.destination().map(|s| s.to_string()),
        "path": h.path().map(|p| p.to_string()),
        "interface": h.interface().map(|s| s.to_string()),
        "member": h.member().map(|s| s.to_string()),
        "serial": h.primary().serial_num().get(),
        "reply_serial": h.reply_serial().map(|s| s.get()),
        "error": h.error_name().map(|s| s.to_string()),
        "signature": m.body().signature().to_string_no_parens(),
        "flags": flags_of(m.primary_header().flags()),
        "ts": epoch_secs(),
        "args": args,
    })
}


/// Does the message originate from (or address) any of the requested services?
/// With no positional services every message passes.
fn matches_service(m: &zbus::Message, services: &[String]) -> bool {
    if services.is_empty() {
        return true;
    }
    let h = m.header();
    let sender = h.sender().map(|s| s.as_str());
    let dest = h.destination().map(|s| s.as_str());
    services
        .iter()
        .any(|svc| Some(svc.as_str()) == sender || Some(svc.as_str()) == dest)
}

/// Parse a short duration string ("250ms", "5s", "1m", or a bare number of
/// seconds). A leading `+`/trailing unit beyond s/ms/us/m/h is rejected.
fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix("us") {
        return Ok(Duration::from_micros(num.parse().map_err(|_| {
            Error::Msg(format!("invalid --timeout: {s}"))
        })?));
    }
    if let Some(num) = s.strip_suffix("ms") {
        return Ok(Duration::from_millis(num.parse().map_err(|_| {
            Error::Msg(format!("invalid --timeout: {s}"))
        })?));
    }
    if let Some(num) = s.strip_suffix('s') {
        return Ok(Duration::from_secs(num.parse().map_err(|_| {
            Error::Msg(format!("invalid --timeout: {s}"))
        })?));
    }
    if let Some(num) = s.strip_suffix('m') {
        return Ok(Duration::from_secs(
            (num.parse::<u64>().map_err(|_| Error::Msg(format!("invalid --timeout: {s}")))?) * 60,
        ));
    }
    // Bare number ⇒ seconds.
    let secs: u64 = s
        .parse()
        .map_err(|_| Error::Msg(format!("invalid --timeout: {s}")))?;
    Ok(Duration::from_secs(secs))
}

/// Implementation of `busx monitor`.
#[allow(clippy::too_many_arguments)]
pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
    json: bool,
    services: Vec<String>,
    interface: Option<String>,
    member: Option<String>,
    path: Option<String>,
    sender: Option<String>,
    raw_match: Option<String>,
    signals: bool,
    limit_messages: Option<u64>,
    timeout: Option<&str>,
) -> Result<()> {
    let conn = connect(user, system, address, verbose)?;

    let rule = crate::dbus::monitor::build_match_rule(
        interface.as_deref(),
        member.as_deref(),
        path.as_deref(),
        sender.as_deref(),
        raw_match.as_deref(),
        signals,
    )?;

    // In signal-only mode we subscribe via the match rule. In all-messages
    // mode we try BecomeMonitor (which sees method_call/return/error too);
    // if the bus refuses it (some daemons/configs disallow monitoring) we
    // fall back to a signal subscription so the command still works.
    if signals {
        // Pure signal subscription; for_match_rule does the right thing.
        let iter = MessageIterator::for_match_rule(rule.clone(), &conn, None)?;
        stream(iter, &services, limit_messages, timeout, json)
    } else {
        match become_monitor(&conn, Some(&rule)) {
            Ok(()) => {
                let iter = MessageIterator::from(&conn);
                stream(iter, &services, limit_messages, timeout, json)
            }
            Err(e) => {
                if verbose {
                    eprintln!(
                        "busx: warning: BecomeMonitor failed ({e}); \
                         falling back to signal subscription"
                    );
                }
                let iter = MessageIterator::for_match_rule(rule.clone(), &conn, None)?;
                stream(iter, &services, limit_messages, timeout, json)
            }
        }
    }
}

/// Ask the bus to send us every message matching `rule` (or all messages if
/// `rule` is `None`). After this returns the connection is a monitor: it can
/// only receive messages, not send them.
fn become_monitor(conn: &zbus::blocking::Connection, rule: Option<&MatchRule<'_>>) -> Result<()> {
    let proxy = zbus::blocking::fdo::MonitoringProxy::new(conn)?;
    let rules: Vec<MatchRule<'_>> = match rule {
        Some(r) => vec![r.clone()],
        None => vec![],
    };
    // The generated blocking proxy consumes `self` here (mirroring the async
    // trait that takes `self`): once BecomeMonitor succeeds the proxy is
    // useless anyway.
    proxy.become_monitor(&rules, 0)?;
    Ok(())
}

/// Drive the iterator, printing each message. In JSON mode that's NDJSON (one
/// object per line); in human mode a multi-line block per message. Honours
/// `--limit-messages` and `--timeout`; whichever triggers first ends the stream.
fn stream(
    iter: MessageIterator,
    services: &[String],
    limit: Option<u64>,
    timeout: Option<&str>,
    json: bool,
) -> Result<()> {
    // `--timeout` is a wall-clock backstop: even with no matching traffic we
    // exit once it elapses. The iterator blocks between messages, so the
    // deadline is re-checked at the top of each iteration.
    let deadline = timeout
        .map(parse_duration)
        .transpose()?
        .map(|d| Instant::now() + d);

    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    let mut count: u64 = 0;
    for msg in iter {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            break;
        }
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                // A single malformed message shouldn't kill the stream.
                eprintln!("busx: warning: dropped message: {e}");
                continue;
            }
        };
        if !matches_service(&msg, services) {
            continue;
        }
        if json {
            let line = serde_json::to_string(&msg_to_json(&msg))?;
            writeln!(out, "{line}")?;
        } else {
            write!(out, "{}", msg_to_human(&msg))?;
        }
        out.flush()?; // line-buffered so a pipe consumer sees each line promptly

        count += 1;
        if let Some(n) = limit {
            if count >= n {
                break;
            }
        }
    }
    Ok(())
}

/// Render a single received message as a `dbus-send`-style human block (spec §10
/// human form). The first line names the type plus sender/destination/path; the
/// second line carries interface/member/serial (and reply_serial or error when
/// present); then each body argument pretty-printed on its own line.
fn msg_to_human(m: &zbus::Message) -> String {
    let h = m.header();
    let ty = match m.message_type() {
        Type::MethodCall => "method_call",
        Type::MethodReturn => "method_return",
        Type::Error => "error",
        Type::Signal => "signal",
    };
    let sender = h.sender().map(|s| s.to_string()).unwrap_or_default();
    let dest = h.destination().map(|s| s.to_string()).unwrap_or_default();
    let path = h.path().map(|p| p.to_string()).unwrap_or_default();
    let iface = h.interface().map(|s| s.to_string()).unwrap_or_default();
    let member = h.member().map(|s| s.to_string()).unwrap_or_default();
    let serial = h.primary().serial_num().get();
    let reply_serial = h.reply_serial().map(|s| s.get());
    let error = h.error_name().map(|s| s.to_string());

    let mut s = String::new();
    s.push_str(ty);
    if !sender.is_empty() {
        s.push_str(&format!("  sender={sender}"));
    }
    if !dest.is_empty() {
        s.push_str(&format!("  →  {dest}"));
    }
    if !path.is_empty() {
        s.push_str(&format!("  path={path}"));
    }
    s.push('\n');
    s.push_str(&format!("  interface={iface}  member={member}  serial={serial}"));
    if let Some(rs) = reply_serial {
        s.push_str(&format!("  reply_serial={rs}"));
    }
    if let Some(e) = &error {
        s.push_str(&format!("  error={e}"));
    }
    s.push('\n');

    // Body args via the same Structure trick as the JSON path, then pretty.
    if let Ok(structure) = m.body().deserialize::<Structure>() {
        for f in structure.fields() {
            s.push_str(&format!("  {}\n", crate::value::pretty::pretty(f)));
        }
    }
    s
}
