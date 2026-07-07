use serde_json::Value;

/// Print compact JSON to stdout (spec §7.2 — never pretty).
pub fn print_json(v: &Value) {
    println!("{}", serde_json::to_string(v).expect("json serialize"));
}
