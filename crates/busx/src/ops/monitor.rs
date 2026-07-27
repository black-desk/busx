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
//! There are two delivery modes, chosen from the resolved match rule's message
//! type:
//!
//! * `--type signal` (a rule whose `type=` is `signal`): a signal subscription.
//!   The match rule is built from the convenience flags and `--match`, then
//!   registered via `MessageStream::for_match_rule`. No privileges needed;
//!   this is what every bus accepts.
//! * Any other `--type` (method_call / method_return / error), or **no
//!   `--type` at all**: the connection becomes a bus monitor via
//!   [`org.freedesktop.DBus.Monitoring.BecomeMonitor`], so it sees every
//!   message crossing the bus — the same mechanism `busctl monitor` uses.
//!   With no filters this is every message; with filters (`--interface`,
//!   `--member`, …) BecomeMonitor applies them at the bus. `BecomeMonitor` is
//!   privileged and may be refused by some bus configurations; when it is, the
//!   command **errors out** rather than silently degrading to signals-only.
//!   Use `--type signal` for plain signal monitoring.

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
    msg_type: Option<Type>,
    limit_messages: Option<u64>,
    timeout: Option<&str>,
) -> Result<()> {
    async_global_executor::block_on(async {
        let conn = dbus::conn::connect(user, system, address).await?;

        // Build the match rule from the convenience flags + `--type`. When the
        // rule pins `type=signal` we use a plain signal subscription (no
        // privileges); anything else — including no `--type` at all — becomes a
        // bus monitor so method calls/returns/errors are visible too.
        let rule = crate::dbus::monitor::build_match_rule(
            interface.as_deref(),
            member.as_deref(),
            path.as_deref(),
            sender.as_deref(),
            raw_match.as_deref(),
            msg_type,
        )?;

        let (stream, monitor_own_name) = if rule.msg_type() == Some(Type::Signal) {
            // Unprivileged signal subscription: every bus accepts it.
            (
                MessageStream::for_match_rule(rule, &conn, None).await?,
                None,
            )
        } else {
            // BecomeMonitor: sees method_call / method_return / error (and
            // signals) crossing the bus — privileged; the bus may refuse it, in
            // which case we error out rather than silently degrading to signals.
            let has_filters = msg_type.is_some()
                || interface.is_some()
                || member.is_some()
                || path.is_some()
                || sender.is_some()
                || raw_match.is_some();
            crate::dbus::monitor::become_monitor(&conn, has_filters.then_some(&rule))
                .await
                .map_err(|e| {
                    crate::error::Error::Msg(format!(
                        "BecomeMonitor refused by the bus ({e}); cannot capture method calls. \
                         Use --type signal for signal-only monitoring (no privileges needed)."
                    ))
                })?;
            // Mirror `busctl monitor`: the daemon emits NameAcquired / NameLost
            // signals for this connection's own unique name as BecomeMonitor
            // takes effect. Capture it so stream_msgs can discard that
            // lifecycle noise until the confirming NameLost(own_name) lands.
            let own_name = conn.unique_name().map(|n| n.to_string());
            (MessageStream::from(&conn), own_name)
        };

        stream_msgs(
            stream,
            &services,
            limit_messages,
            timeout,
            json,
            monitor_own_name,
        )
        .await
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
    monitor_own_name: Option<String>,
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

    // In BecomeMonitor mode (`monitor_own_name` set), mirror `busctl monitor`:
    // the daemon emits NameAcquired / NameLost signals for this connection's
    // own unique name as BecomeMonitor takes effect — bus plumbing the user
    // didn't ask for. Discard everything until the confirming
    // `NameLost(own_name)` lands, then forward normally. In signal-subscription
    // mode (`None`) there's no such noise, so we start already "ready".
    let mut monitor_ready = monitor_own_name.is_none();

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
                    if !monitor_ready {
                        if let Some(name) = &monitor_own_name
                            && crate::dbus::monitor::is_monitor_ready_signal(&msg, name)
                        {
                            monitor_ready = true;
                        }
                        continue;
                    }
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
