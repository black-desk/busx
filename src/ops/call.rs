//! `busx call` — invoke a D-Bus method and print its return values as
//! type-tagged JSON (spec §7).
//!
//! The positional arguments are encoded by [`crate::value::encode`] (busctl
//! signature + tokens), the call is issued through a generic
//! [`zbus::blocking::Proxy`], and every return value is rendered via
//! [`crate::value::decode::to_tagged`].

use crate::conn::connect;
use crate::error::Result;
use serde_json::json;
use zvariant::{Structure, StructureBuilder};

/// Implementation of `busx call`.
///
/// `args[0]` is the busctl-style signature string; the rest are values.
#[allow(clippy::too_many_arguments)]
pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
    json: bool,
    service: &str,
    object: &str,
    interface: &str,
    method: &str,
    args: &[String],
) -> Result<()> {
    let conn = connect(user, system, address, verbose)?;
    let proxy = zbus::blocking::Proxy::new(&conn, service, object, interface)?;

    // Encode the positional args into N `Value`s.
    let values = crate::value::encode::parse(args)?;

    // Build the outgoing body. `Proxy::call_method` wants a `Serialize +
    // DynamicType`; a `Structure` carries the concatenated signature of its
    // fields, so the peer sees exactly N positional arguments. The empty-arg
    // case is handled separately (an empty `Structure` is not constructible).
    let reply = if values.is_empty() {
        proxy.call_method(method, &())?
    } else {
        let mut builder = StructureBuilder::new();
        for v in values {
            builder = builder.append_field(v);
        }
        let body = builder.build()?;
        proxy.call_method(method, &body)?
    };

    // Read every return value out of the reply body. `Body::deserialize::<
    // Structure>()` accepts any body signature (wrapping a single value in a
    // one-field struct), so it works for 0/1/N return values uniformly.
    let body = reply.body();
    let fields: Vec<zvariant::Value<'_>> = if body.is_empty() {
        Vec::new()
    } else {
        let structure: Structure = body.deserialize()?;
        structure.fields().to_vec()
    };

    if json {
        let out: Vec<_> = fields.iter().map(crate::value::decode::to_tagged).collect();
        crate::out::print_json(&json!(out));
    } else {
        for f in &fields {
            println!("{}  {}", f.value_signature(), crate::value::pretty::pretty(f));
        }
    }
    Ok(())
}
