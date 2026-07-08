// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Monitor support shared by the CLI `monitor` op and the future TUI monitor
//! (spec §10). For now only the pure match-rule builder lives here; the async
//! streaming + `BusMessage` decoding arrive with the TUI monitor phase.

use crate::error::{Error, Result};
use zbus::message::Type;
use zbus::MatchRule;

/// Assemble the match rule from the convenience flags (+`type='signal'` when
/// `signals`). `raw_match`, if given, is parsed directly and overrides the
/// builder path so users get exactly what they typed.
pub fn build_match_rule(
    interface: Option<&str>,
    member: Option<&str>,
    path: Option<&str>,
    sender: Option<&str>,
    raw_match: Option<&str>,
    signals: bool,
) -> Result<MatchRule<'static>> {
    if let Some(raw) = raw_match {
        return MatchRule::try_from(raw)
            .map(|r| r.into_owned())
            .map_err(|e| Error::Msg(format!("invalid --match rule: {e}")));
    }

    let mut builder = MatchRule::builder();
    if signals {
        builder = builder.msg_type(Type::Signal);
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
