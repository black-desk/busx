use crate::conn::connect;
use crate::error::Result;
use serde_json::json;
use zbus::blocking::fdo::DBusProxy;

/// `busx list` — print the names on the bus as a JSON array (spec §7).
///
/// Flags:
/// - `--activatable`: list *activatable* names instead of currently-owned ones.
/// - `--unique`: keep only unique (`:`-prefixed) names.
/// - `--acquired`: keep only well-known (non-unique) names.
///
/// `--unique` and `--acquired` are mutually exclusive filters; if both are
/// given they cancel out and all names are returned (matching the documented
/// "either-or" semantics — both set means no filtering).
pub fn run(
    user: bool,
    system: bool,
    address: Option<&str>,
    verbose: bool,
    unique: bool,
    acquired: bool,
    activatable: bool,
) -> Result<()> {
    let conn = connect(user, system, address, verbose)?;
    let dbus = DBusProxy::new(&conn)?;
    let mut names: Vec<String> = if activatable {
        dbus.list_activatable_names()?
    } else {
        dbus.list_names()?
    }
    .into_iter()
    .map(|n| n.to_string())
    .collect();
    if unique && !acquired {
        names.retain(|n| n.starts_with(':'));
    } else if acquired && !unique {
        names.retain(|n| !n.starts_with(':'));
    }
    names.sort();
    crate::out::print_json(&json!(names));
    Ok(())
}
