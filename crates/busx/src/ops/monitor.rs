// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx monitor` — stream bus messages as NDJSON, with match-rule filtering.
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
//! * Default (no `--all`): a signal subscription. A D-Bus match rule — type
//!   pinned to `signal` — is built from the convenience flags and `--match`,
//!   then registered via `MessageStream::for_match_rule`. No privileges needed;
//!   this is what every bus accepts.
//! * `--all`: the connection is converted into a bus monitor via
//!   [`org.freedesktop.DBus.Monitoring.BecomeMonitor`], so it sees every message
//!   crossing the bus (method_call / method_return / error / signal) — the same
//!   mechanism `busctl monitor` uses. `BecomeMonitor` is privileged and may be
//!   refused by some bus configurations; when it is, the command **errors out**
//!   (the user explicitly asked for all message types) rather than silently
//!   degrading to signals-only. Drop `--all` for plain signal monitoring.

use crate::dbus;
use crate::error::{Error, Result};
use futures::future::OptionFuture;
use futures::{FutureExt, StreamExt};
use serde_json::{Value as Json, json};
use std::io::{BufWriter, Write};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zbus::MessageStream;
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

/// Render a single received message as the  JSON object.
///
/// The body is decoded as a `Structure` (a trick: this accepts any body
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
        .map(|s| {
            s.fields()
                .iter()
                .map(crate::value::decode::to_tagged)
                .collect()
        })
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

/// Parse a short duration string: `250us`, `250ms`, `5s`, `1m`, or a bare
/// number of seconds. Any other suffix (e.g. `h`), or a leading sign, is
/// rejected.
fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix("us") {
        return Ok(Duration::from_micros(
            num.parse()
                .map_err(|_| Error::Msg(format!("invalid --timeout: {s}")))?,
        ));
    }
    if let Some(num) = s.strip_suffix("ms") {
        return Ok(Duration::from_millis(
            num.parse()
                .map_err(|_| Error::Msg(format!("invalid --timeout: {s}")))?,
        ));
    }
    if let Some(num) = s.strip_suffix('s') {
        return Ok(Duration::from_secs(
            num.parse()
                .map_err(|_| Error::Msg(format!("invalid --timeout: {s}")))?,
        ));
    }
    if let Some(num) = s.strip_suffix('m') {
        return Ok(Duration::from_secs(
            (num.parse::<u64>()
                .map_err(|_| Error::Msg(format!("invalid --timeout: {s}")))?)
                * 60,
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
    json: bool,
    services: Vec<String>,
    interface: Option<String>,
    member: Option<String>,
    path: Option<String>,
    sender: Option<String>,
    raw_match: Option<String>,
    all: bool,
    limit_messages: Option<u64>,
    timeout: Option<&str>,
) -> Result<()> {
    async_global_executor::block_on(async {
        let conn = dbus::conn::connect(user, system, address).await?;

        // Default = signal subscription (a plain match rule; type pinned to
        // Signal). --all instead requests BecomeMonitor so method calls/returns/
        // errors are visible too — a privileged op the bus may refuse; when it
        // does we error out (the user explicitly asked for methods) and never
        // silently degrade to signals-only.
        let rule = crate::dbus::monitor::build_match_rule(
            interface.as_deref(),
            member.as_deref(),
            path.as_deref(),
            sender.as_deref(),
            raw_match.as_deref(),
            if all { None } else { Some(Type::Signal) },
        )?;

        let stream = if all {
            dbus::monitor::become_monitor(&conn, Some(&rule))
                .await
                .map_err(|e| {
                    crate::error::Error::Msg(format!(
                        "BecomeMonitor refused by the bus ({e}); cannot capture method calls. \
                         Omit --all for signal-only monitoring."
                    ))
                })?;
            MessageStream::from(&conn)
        } else {
            MessageStream::for_match_rule(rule.clone(), &conn, None).await?
        };

        stream_msgs(stream, &services, limit_messages, timeout, json).await
    })
}

/// Drive the stream, printing each message. In JSON mode that's NDJSON (one
/// object per line); in human mode a multi-line block per message. Honours
/// `--limit-messages` and `--timeout`; whichever triggers first ends the
/// stream.
///
/// `--timeout` is a wall-clock backstop: the stream is raced against a timer
/// future via `select!`, so the timeout fires even when no matching traffic
/// arrives. (The old blocking `MessageIterator::next()` dead-waited, so its
/// deadline check — inside the loop body — only ran after a message landed,
/// making `--timeout` hang forever on an idle bus.)
async fn stream_msgs(
    stream: MessageStream,
    services: &[String],
    limit: Option<u64>,
    timeout: Option<&str>,
    json: bool,
) -> Result<()> {
    let deadline = timeout.map(parse_duration).transpose()?;

    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    // `OptionFuture` wraps `Option<Future>`: `Some(timer)` resolves when the
    // timeout elapses and breaks the loop; `None` (no `--timeout`) is pending
    // forever, so the `select!` arm never fires — one loop body covers both
    // cases. The blocking `become_monitor` that used to live here is gone; the
    // async `dbus::monitor::become_monitor` is reused instead.
    let mut timer = OptionFuture::from(deadline.map(async_io::Timer::after)).fuse();
    let mut stream = stream.fuse();

    let mut count: u64 = 0;
    loop {
        futures::select! {
            msg = stream.next() => match msg {
                None => break,
                Some(Err(e)) => {
                    // A single malformed message shouldn't kill the stream.
                    tracing::debug!("dropped malformed message: {e}");
                    continue;
                }
                Some(Ok(msg)) => {
                    if !matches_service(&msg, services) {
                        continue;
                    }
                    if json {
                        let line = serde_json::to_string(&msg_to_json(&msg))?;
                        writeln!(out, "{line}")?;
                    } else {
                        write!(out, "{}", crate::dbus::monitor::format_message(&msg))?;
                    }
                    out.flush()?; // line-buffered so a pipe consumer sees each line promptly

                    count += 1;
                    if let Some(n) = limit
                        && count >= n
                    {
                        break;
                    }
                }
            },
            _ = timer => break, // `--timeout` elapsed
        }
    }
    Ok(())
}
