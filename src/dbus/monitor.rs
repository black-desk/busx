// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Monitor support shared by the CLI `monitor` op and the the TUI monitor
//!. The pure match-rule builder and the `dbus-send`-style message
//! formatter (`format_message`) live here; the async `become_monitor` mirrors
//! the blocking CLI op. The async streaming + `BusMessage` decoding arrive with
//! the TUI monitor phase.

use crate::error::{Error, Result};
use zbus::MatchRule;
use zbus::fdo::MonitoringProxy;
use zbus::message::Type;
use zvariant::Structure;

/// Assemble the match rule from the convenience fields. `msg_type`, if given,
/// pins the rule's `type=` (e.g. `Signal` for a pure signal subscription,
/// `MethodCall` for a TUI method listen). `raw_match`, if given, is parsed
/// directly and overrides the builder path so users get exactly what they typed.
pub fn build_match_rule(
    interface: Option<&str>,
    member: Option<&str>,
    path: Option<&str>,
    sender: Option<&str>,
    raw_match: Option<&str>,
    msg_type: Option<Type>,
) -> Result<MatchRule<'static>> {
    if let Some(raw) = raw_match {
        return MatchRule::try_from(raw)
            .map(|r| r.into_owned())
            .map_err(|e| Error::Msg(format!("invalid --match rule: {e}")));
    }

    let mut builder = MatchRule::builder();
    if let Some(ty) = msg_type {
        builder = builder.msg_type(ty);
    }
    // `sender` matches the origin of the message; for a well-known service that
    // is its unique name, but the bus also accepts the well-known name here.
    if let Some(s) = sender {
        builder = builder.sender(s)?;
    }
    if let Some(iface) = interface {
        builder = builder.interface(iface)?;
    }
    if let Some(mem) = member {
        builder = builder.member(mem)?;
    }
    if let Some(p) = path {
        builder = builder.path(p)?;
    }
    // Positional services don't map cleanly onto a single match-rule field, so
    // they are applied as client-side filtering after receipt (see the CLI
    // `matches_service`). The convenience flags above do go into the rule.
    Ok(builder.build().into_owned())
}

/// Async `BecomeMonitor`. After this the connection only receives messages
/// (it can no longer send). Mirrors the blocking `ops::monitor::become_monitor`.
pub async fn become_monitor(conn: &zbus::Connection, rule: Option<&MatchRule<'_>>) -> Result<()> {
    let proxy = MonitoringProxy::new(conn).await?;
    let rules: Vec<MatchRule<'_>> = match rule {
        Some(r) => vec![r.clone()],
        None => vec![],
    };
    proxy.become_monitor(&rules, 0).await?;
    Ok(())
}

/// Render a single received message as a `dbus-send`-style human block (
/// human form). The first line names the type plus sender/destination/path; the
/// second line carries interface/member/serial (and reply_serial or error when
/// present); then each body argument pretty-printed on its own line.
pub fn format_message(m: &zbus::Message) -> String {
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
    s.push_str(&format!(
        "  interface={iface}  member={member}  serial={serial}"
    ));
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
