// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Render a busx operation as another D-Bus tool's command line.
//!
//! Pure: no IO, no clipboard — the copy-as popup calls
//! [`generate`]. busx's method/property-set args are **busctl-style**
//! (`crate::value::encode`): a separate signature string plus positional
//! value tokens (basic → one token; variant → inner-sig+value; array →
//! count+N; dict → count+N pairs; struct → flat). So [`Tool::Busctl`]
//! copy-as is **1:1** — the signature and tokens map directly onto
//! `busctl call`/`set-property`. The other tools **walk the signature and
//! tokens recursively** (like the encoder) and render each tool's
//! complex-type CLI syntax:
//!
//! - **gdbus** — every type → GVariant text (`[…]`, `{k:v}`, `(…)`, `<…>`),
//!   complete; no notes.
//! - **dbus-send** — `array:`/`dict:`/`variant:` for basic-element
//!   containers per `man dbus-send`; structs/nested containers get an honest
//!   `# note` (dbus-send forbids nested containers and has no struct syntax).
//! - **qdbus** — basic values and simple arrays positionally; dicts/structs
//!   get an honest `# note` (qdbus has no positional container CLI syntax).
//!
//! Where a tool genuinely can't express a type, copy-as emits an honest
//! `# note` rather than a command the tool would reject (no broken values).
//!
//! `--user`/`--system` bus flags are intentionally omitted (the user adds
//! the bus they want); for gdbus we emit the placeholder `--session`.

/// An operation we can render as another tool's command.
#[derive(Clone, Debug)]
pub enum CopyOp {
    /// Method call. `signature` is the method's IN-signature; `args` are
    /// busctl-style value tokens laid out per [`crate::value::encode`].
    Call {
        service: String,
        object: String,
        iface: String,
        method: String,
        signature: String,
        args: Vec<String>,
    },
    /// Property get.
    Get {
        service: String,
        object: String,
        iface: String,
        property: String,
    },
    /// Property set. `signature` + `value` are busctl-style (one value's
    /// worth of tokens).
    Set {
        service: String,
        object: String,
        iface: String,
        property: String,
        signature: String,
        value: Vec<String>,
    },
    /// Signal/property/method listen. `rule` is the D-Bus match-rule string.
    Listen { rule: String },
}

/// The other D-Bus tool to render as. Listed in [`Tool::ALL`] in popup order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    DbusSend,
    Busctl,
    Qdbus,
    Gdbus,
}

impl Tool {
    /// All tools, in popup-display order.
    pub const ALL: [Tool; 4] = [Tool::DbusSend, Tool::Busctl, Tool::Qdbus, Tool::Gdbus];

    /// The tool's command name (e.g. `"dbus-send"`).
    pub fn name(self) -> &'static str {
        match self {
            Self::DbusSend => "dbus-send",
            Self::Busctl => "busctl",
            Self::Qdbus => "qdbus",
            Self::Gdbus => "gdbus",
        }
    }
}

/// Render `op` as `tool`'s command line.
///
/// Returns `None` only where the tool genuinely cannot express the operation
/// (qdbus has no monitor). Best-effort outputs carry a trailing `# …` note
/// where a tool can't fully express a signature.
pub fn generate(op: &CopyOp, tool: Tool) -> Option<String> {
    match op {
        CopyOp::Call {
            service,
            object,
            iface,
            method,
            signature,
            args,
        } => call(service, object, iface, method, signature, args, tool),
        CopyOp::Get {
            service,
            object,
            iface,
            property,
        } => Some(get(service, object, iface, property, tool)),
        CopyOp::Set {
            service,
            object,
            iface,
            property,
            signature,
            value,
        } => set(service, object, iface, property, signature, value, tool),
        CopyOp::Listen { rule } => listen(rule, tool),
    }
}

// --- method call -----------------------------------------------------------

#[allow(clippy::too_many_arguments)]
fn call(
    service: &str,
    object: &str,
    iface: &str,
    method: &str,
    signature: &str,
    args: &[String],
    tool: Tool,
) -> Option<String> {
    match tool {
        // 1:1 — the args are already busctl tokens. Quote each and space-join.
        Tool::Busctl => {
            let mut parts: Vec<String> = vec![
                "busctl".into(),
                "call".into(),
                service.into(),
                object.into(),
                iface.into(),
                method.into(),
            ];
            if !signature.is_empty() {
                parts.push(signature.into());
            }
            for a in args {
                parts.push(quote(a));
            }
            Some(parts.join(" "))
        }
        Tool::DbusSend => {
            let mut out =
                format!("dbus-send --print-reply --dest={service} {object} {iface}.{method}");
            let (rendered, notes) = render_args(signature, args, tool);
            for r in &rendered {
                if r.unsupported.is_none() && !(r.quoted && r.text.is_empty()) {
                    out.push(' ');
                    out.push_str(&r.text);
                }
            }
            // If any arg is unsupported, dbus-send can't express this op → grey
            // it out in the popup (None) rather than showing a broken command.
            if notes.is_empty() { Some(out) } else { None }
        }
        Tool::Qdbus => {
            let mut out = format!("qdbus {service} {object} {iface}.{method}");
            let (rendered, notes) = render_args(signature, args, tool);
            for r in &rendered {
                if r.unsupported.is_none() && !(r.quoted && r.text.is_empty()) {
                    out.push(' ');
                    out.push_str(&append_rendered(r));
                }
            }
            if notes.is_empty() { Some(out) } else { None }
        }
        Tool::Gdbus => {
            let mut out = format!(
                "gdbus call --session --dest {service} --object-path {object} --method {iface}.{method}"
            );
            let (rendered, _notes) = render_args(signature, args, tool);
            // gdbus can express every type, so there are never notes.
            for r in &rendered {
                out.push(' ');
                out.push_str(&r.text);
            }
            Some(out)
        }
    }
}

// --- property get ----------------------------------------------------------

fn get(service: &str, object: &str, iface: &str, property: &str, tool: Tool) -> String {
    match tool {
        Tool::Busctl => format!("busctl get-property {service} {object} {iface} {property}"),
        Tool::DbusSend => format!(
            "dbus-send --print-reply --dest={service} {object} \
             org.freedesktop.DBus.Properties.Get string:{iface} string:{property}"
        ),
        // qdbus reads a property via the standard Properties.Get method
        // (qdbus has no first-class property syntax on the command line);
        // `--literal` returns the raw value without qdbus's pretty-printer.
        Tool::Qdbus => format!(
            "qdbus --literal {service} {object} org.freedesktop.DBus.Properties.Get {iface} {property}"
        ),
        // gdbus Properties.Get takes two GVariant string args; gdbus infers
        // the `'s'` type from the known signature `(ss)`, so bare quoted
        // strings work.
        Tool::Gdbus => format!(
            "gdbus call --session --dest {service} --object-path {object} \
             --method org.freedesktop.DBus.Properties.Get \"{iface}\" \"{property}\""
        ),
    }
}

// --- property set ----------------------------------------------------------

fn set(
    service: &str,
    object: &str,
    iface: &str,
    property: &str,
    signature: &str,
    value: &[String],
    tool: Tool,
) -> Option<String> {
    match tool {
        // 1:1 — signature + value tokens map straight onto set-property.
        Tool::Busctl => {
            let mut parts: Vec<String> = vec![
                "busctl".into(),
                "set-property".into(),
                service.into(),
                object.into(),
                iface.into(),
                property.into(),
                signature.into(),
            ];
            for v in value {
                parts.push(quote(v));
            }
            Some(parts.join(" "))
        }
        Tool::DbusSend => {
            // dbus-send Properties.Set variant inner type must be basic.
            let mut tok = Tok {
                toks: value,
                pos: 0,
            };
            match dbus_send_basic_tag(signature) {
                Some(tag) => {
                    let val = tok.next();
                    Some(format!(
                        "dbus-send --print-reply --dest={service} {object} \
                         org.freedesktop.DBus.Properties.Set string:{iface} string:{property} variant:{tag}:{val}"
                    ))
                }
                None => {
                    // Non-basic property type — dbus-send can't express it.
                    drain_value(&mut tok, signature);
                    None
                }
            }
        }
        // qdbus Properties.Set: `variant:` takes a single basic value.
        Tool::Qdbus => {
            let mut tok = Tok {
                toks: value,
                pos: 0,
            };
            if dbus_send_basic_literal_kind(signature).is_some() {
                let val = tok.next();
                Some(format!(
                    "qdbus {service} {object} org.freedesktop.DBus.Properties.Set \
                     {iface} {property} variant:{val}"
                ))
            } else {
                drain_value(&mut tok, signature);
                None
            }
        }
        // gdbus Properties.Set: GVariant variant value. gdbus can express every type.
        Tool::Gdbus => {
            let mut tok = Tok {
                toks: value,
                pos: 0,
            };
            let inner = gdbus_value(signature, &mut tok).text;
            let gv = format!("<{inner}>");
            Some(format!(
                "gdbus call --session --dest {service} --object-path {object} \
                 --method org.freedesktop.DBus.Properties.Set \
                 \"{iface}\" \"{property}\" {gv}"
            ))
        }
    }
}

// --- listen ----------------------------------------------------------------

fn listen(rule: &str, tool: Tool) -> Option<String> {
    match tool {
        Tool::DbusSend => Some(format!("dbus-monitor {}", quote(rule))),
        Tool::Busctl => Some(format!("busctl monitor {}", quote(rule))),
        // qdbus has no monitor facility at all.
        Tool::Qdbus => None,
        // gdbus monitor is unfiltered (it ignores match rules), so emit the
        // bare command plus a note rather than dropping the user.
        Tool::Gdbus => Some(
            "gdbus monitor --session\n# gdbus monitor is unfiltered — it ignores the rule".into(),
        ),
    }
}

/// Final shell form of a rendered arg: if `quoted`, it is already shell-ready
/// (e.g. a qdbus array expanded into individually-quoted positional args);
/// otherwise apply [`quote`].
fn append_rendered(r: &Rendered) -> String {
    if r.quoted {
        r.text.clone()
    } else {
        quote(&r.text)
    }
}

// --- shared recursive token walker -----------------------------------------
//
// The three converters (dbus-send / qdbus / gdbus) all walk the signature the
// same way the encoder does (`crate::value::encode`): one complete type is
// pulled out of the signature stream while its matching busctl tokens are
// consumed positionally —
//
//   basic → 1 token;  `v` → inner-sig token + its value tokens;
//   `a<X>` → count `N` + N elements;  `a{KV}` → count `N` + N (key,value) pairs;
//   `(...)` → fields flat until `)`.
//
// Walking **tokens** (rather than `encode::parse`→`Value`) keeps copy-as robust
// to partial/unfilled args: a token shortage renders a `?` placeholder instead
// of erroring, so the user still gets a usable skeleton command. Each per-tool
// renderer renders the consumed value in that tool's CLI syntax.

/// Placeholder emitted when a token is missing (partial/unfilled args).
const MISSING: &str = "?";

/// Read-only cursor over the positional busctl tokens. [`Tok::next`] never
/// panics: once the tokens run out it returns [`MISSING`] for every subsequent
/// read, so partial args render a usable skeleton.
struct Tok<'a> {
    toks: &'a [String],
    pos: usize,
}

impl<'a> Tok<'a> {
    fn next(&mut self) -> &'a str {
        let t = self
            .toks
            .get(self.pos)
            .map(String::as_str)
            .unwrap_or(MISSING);
        if self.pos < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    /// Read a count token (busctl arrays/dicts prefix their elements with `N`).
    /// On a missing/unparseable token, returns 0 (renders an empty container).
    fn next_count(&mut self) -> usize {
        self.next().parse::<usize>().unwrap_or(0)
    }
}

/// The result of rendering one value for a tool. `unsupported` carries the
/// signature substring the tool cannot express (if any), so the caller can
/// attach an honest `# note` instead of emitting a malformed command.
struct Rendered {
    /// The rendered value text.
    text: String,
    /// When `true`, [`text`](Self::text) is already final shell form (e.g. a
    /// qdbus array expanded into several space-joined, individually-quoted
    /// positional args) and the caller must append it verbatim without
    /// re-quoting. When `false` the caller applies its own quoting.
    quoted: bool,
    /// `Some(sig)` if (part of) this value's type is beyond the tool's syntax.
    unsupported: Option<String>,
}

impl Rendered {
    fn ok(text: String) -> Self {
        Self {
            text,
            quoted: false,
            unsupported: None,
        }
    }
    fn unsupported(sig: impl Into<String>) -> Self {
        Self {
            text: MISSING.into(),
            quoted: false,
            unsupported: Some(sig.into()),
        }
    }
}

/// Render every top-level arg of a signature for `tool`, returning one
/// rendered string per arg plus the union of any unsupported-type notes.
fn render_args(signature: &str, args: &[String], tool: Tool) -> (Vec<Rendered>, Vec<String>) {
    let types = split_signature(signature);
    let mut tok = Tok { toks: args, pos: 0 };
    let mut rendered = Vec::with_capacity(types.len());
    let mut notes: Vec<String> = Vec::new();
    for ty in &types {
        let r = match tool {
            Tool::DbusSend => dbus_send_value(ty, &mut tok),
            Tool::Qdbus => qdbus_value(ty, &mut tok),
            Tool::Gdbus => gdbus_value(ty, &mut tok),
            // busctl is 1:1 and handled separately; never reached here.
            Tool::Busctl => Rendered::ok(tok.next().into()),
        };
        if let Some(n) = &r.unsupported {
            notes.push(n.clone());
        }
        rendered.push(r);
    }
    (rendered, notes)
}

// === dbus-send renderer ===
//
// From `man dbus-send` (D-Bus 1.14.10):
//   <item>       ::= <type>:<value>
//   <array>      ::= array:<type>:<value>[,<value>...]
//   <dict>       ::= dict:<type>:<type>:<key>,<value>[,<key>,<value>...]
//   <variant>    ::= variant:<type>:<value>
//   <type>       ::= string|int16|uint16|int32|uint32|int64|uint64|double|
//                    byte|boolean|objpath
//
// "dbus-send does not permit empty containers or nested containers (e.g. arrays
// of variants)." Structs are not in the BNF at all. So dbus-send can express
// exactly: basic values, variants whose inner type is basic, arrays of basic
// elements, and dicts of basic→basic. Anything else → unsupported (honest note).

fn dbus_send_value(ty: &str, tok: &mut Tok<'_>) -> Rendered {
    // Basic type → `type:value`.
    if let Some(tag) = dbus_send_basic_tag(ty) {
        return Rendered::ok(format!("{tag}:{}", tok.next()));
    }
    match ty {
        // variant:<innertype>:<value> — only if the inner type is basic.
        "v" => {
            let inner = tok.next();
            match dbus_send_basic_tag(inner) {
                Some(itag) => {
                    let val = tok.next();
                    Rendered::ok(format!("variant:{itag}:{val}"))
                }
                None => {
                    // dbus-send can't nest a complex-typed variant. Drain the
                    // inner value's tokens so subsequent args stay aligned; the
                    // rendered arg is dropped in favor of a `# note`.
                    drain_value(tok, inner);
                    Rendered::unsupported(format!("v<{inner}>"))
                }
            }
        }
        // array:<elemtype>:<v1>,<v2>,… — only if the element type is basic.
        t if t.starts_with('a') && !t.starts_with("a{") => {
            let elem = &t[1..];
            match dbus_send_basic_tag(elem) {
                Some(etag) => {
                    let n = tok.next_count();
                    // dbus-send forbids empty containers (`man dbus-send`).
                    if n == 0 {
                        return Rendered::unsupported(t.to_string());
                    }
                    let mut parts = format!("array:{etag}:");
                    for i in 0..n {
                        if i > 0 {
                            parts.push(',');
                        }
                        parts.push_str(tok.next());
                    }
                    Rendered::ok(parts)
                }
                None => {
                    // Nested/non-basic element → dbus-send can't express it.
                    // Drain the element tokens best-effort (count + N elements)
                    // so subsequent args stay roughly aligned.
                    drain_array(tok, elem);
                    Rendered::unsupported(t.to_string())
                }
            }
        }
        // dict:<keytype>:<valtype>:<key>,<value>,… — only basic→basic.
        t if t.starts_with("a{") => {
            let inner = &t["a{".len()..t.len() - 1]; // strip a{ and trailing }
            let chars: Vec<char> = inner.chars().collect();
            let mut pos = 0;
            if skip_complete_type(&chars, &mut pos).is_err() {
                return Rendered::unsupported(t.to_string());
            }
            let key_sig: String = chars[..pos].iter().collect();
            let val_start = pos;
            if skip_complete_type(&chars, &mut pos).is_err() {
                return Rendered::unsupported(t.to_string());
            }
            let val_sig: String = chars[val_start..pos].iter().collect();
            match (dbus_send_basic_tag(&key_sig), dbus_send_basic_tag(&val_sig)) {
                (Some(ktag), Some(vtag)) => {
                    let n = tok.next_count();
                    // dbus-send forbids empty containers (`man dbus-send`).
                    if n == 0 {
                        return Rendered::unsupported(t.to_string());
                    }
                    let mut parts = format!("dict:{ktag}:{vtag}:");
                    for i in 0..n {
                        if i > 0 {
                            parts.push(',');
                        }
                        parts.push_str(tok.next());
                        parts.push(',');
                        parts.push_str(tok.next());
                    }
                    Rendered::ok(parts)
                }
                _ => {
                    drain_dict(tok, &key_sig, &val_sig);
                    Rendered::unsupported(t.to_string())
                }
            }
        }
        // struct "(…)" — not expressible by dbus-send.
        t if t.starts_with('(') => {
            drain_struct(tok, t);
            Rendered::unsupported(t.to_string())
        }
        // Unknown type — best-effort, treat as a single token.
        _ => Rendered::ok(tok.next().into()),
    }
}

/// The dbus-send `<type>` tag for a basic type code (`s`→`string`,
/// `u`→`uint32`, …), or `None` for complex/unsupported codes. This is the
/// dbus-send BNF's `<type>` set (note: no `signature`/`variant`).
fn dbus_send_basic_tag(sig: &str) -> Option<&'static str> {
    match sig {
        "s" => Some("string"),
        "u" => Some("uint32"),
        "i" => Some("int32"),
        "t" => Some("uint64"),
        "x" => Some("int64"),
        "n" => Some("int16"),
        "q" => Some("uint16"),
        "y" => Some("byte"),
        "b" => Some("boolean"),
        "d" => Some("double"),
        "o" => Some("objpath"),
        _ => None,
    }
}

/// Advance `tok` past all the tokens of an `elem`-typed array (count + N
/// elements), so subsequent args stay aligned when dbus-send can't express it.
fn drain_array(tok: &mut Tok<'_>, elem: &str) {
    let n = tok.next_count();
    for _ in 0..n {
        drain_value(tok, elem);
    }
}

/// Advance `tok` past all the tokens of a `key`/`val`-typed dict.
fn drain_dict(tok: &mut Tok<'_>, key: &str, val: &str) {
    let n = tok.next_count();
    for _ in 0..n {
        drain_value(tok, key);
        drain_value(tok, val);
    }
}

/// Advance `tok` past all the tokens of a struct (fields flat until `)`).
fn drain_struct(tok: &mut Tok<'_>, ty: &str) {
    let inner = &ty[1..ty.len() - 1];
    let chars: Vec<char> = inner.chars().collect();
    let mut pos = 0;
    while pos < chars.len() {
        let start = pos;
        if skip_complete_type(&chars, &mut pos).is_err() {
            break;
        }
        let field: String = chars[start..pos].iter().collect();
        drain_value(tok, &field);
    }
}

/// Advance `tok` past all tokens belonging to one value of type `ty`, mirroring
/// the encoder's token layout. Used only when *discarding* an inexpressible
/// value (to keep subsequent args aligned); the rendered output is a `# note`.
fn drain_value(tok: &mut Tok<'_>, ty: &str) {
    if dbus_send_basic_tag(ty).is_some() || ty == "g" {
        tok.next();
        return;
    }
    match ty {
        "v" => {
            let inner = tok.next();
            drain_value(tok, inner);
        }
        t if t.starts_with('a') && !t.starts_with("a{") => {
            drain_array(tok, &t[1..]);
        }
        t if t.starts_with("a{") => {
            let inner = &t["a{".len()..t.len() - 1];
            let chars: Vec<char> = inner.chars().collect();
            let mut pos = 0;
            if skip_complete_type(&chars, &mut pos).is_err() {
                return;
            }
            let key: String = chars[..pos].iter().collect();
            let vstart = pos;
            if skip_complete_type(&chars, &mut pos).is_err() {
                return;
            }
            let val: String = chars[vstart..pos].iter().collect();
            drain_dict(tok, &key, &val);
        }
        t if t.starts_with('(') => drain_struct(tok, t),
        _ => {
            tok.next();
        }
    }
}

// === qdbus renderer ===
//
// qdbus infers types from introspection and accepts positional values. It has
// no first-class CLI syntax for containers (Qt parses args via QDBusArgument
// from the introspected signature). On the command line qdbus handles basic
// values reliably; arrays of strings can be approximated by passing each
// element as a separate positional arg (qdbus fills them into the array). For
// dicts/structs/variants qdbus cannot reliably express the value positionally —
// render an honest note. (This is a CLI limitation; programmatic QDBus handles
// all types, but copy-as is about the command line.)

fn qdbus_value(ty: &str, tok: &mut Tok<'_>) -> Rendered {
    // Basic type → the literal token (qdbus infers the type).
    if dbus_send_basic_literal_kind(ty).is_some() {
        return Rendered::ok(tok.next().into());
    }
    match ty {
        "v" => {
            // Consume the busctl inner-signature token, then the value token.
            // qdbus treats `variant:<value>` specially for variant args.
            let _inner_sig = tok.next();
            let val = tok.next();
            Rendered::ok(format!("variant:{val}"))
        }
        // array (non-dict): busctl lays out `count elem0 elem1 …`. Drop the
        // count, pass each element as a separate positional value — works for
        // arrays of basic types qdbus can infer. Each element is individually
        // quoted and the whole run is marked `quoted` so the caller appends it
        // verbatim (it already spans multiple space-joined shell words).
        t if t.starts_with('a') && !t.starts_with("a{") => {
            let elem = &t[1..];
            if dbus_send_basic_literal_kind(elem).is_some() {
                let n = tok.next_count();
                let mut parts = Vec::with_capacity(n);
                for _ in 0..n {
                    parts.push(quote(tok.next()));
                }
                let mut r = Rendered::ok(parts.join(" "));
                r.quoted = true;
                r
            } else {
                drain_array(tok, elem);
                Rendered::unsupported(t.to_string())
            }
        }
        // dict / struct → qdbus can't express positionally; honest note.
        t if t.starts_with("a{") || t.starts_with('(') => {
            if t.starts_with("a{") {
                let inner = &t["a{".len()..t.len() - 1];
                let chars: Vec<char> = inner.chars().collect();
                let mut pos = 0;
                if skip_complete_type(&chars, &mut pos).is_ok() {
                    let key: String = chars[..pos].iter().collect();
                    let vstart = pos;
                    if skip_complete_type(&chars, &mut pos).is_ok() {
                        let val: String = chars[vstart..pos].iter().collect();
                        drain_dict(tok, &key, &val);
                    }
                }
            } else {
                drain_struct(tok, t);
            }
            Rendered::unsupported(t.to_string())
        }
        _ => Rendered::ok(tok.next().into()),
    }
}

// === gdbus renderer ===
//
// From `man gdbus`: "Each argument to pass to the method must be specified as a
// serialized GVariant except that strings do not need explicit quotes." GVariant
// text format can express every D-Bus type — so gdbus rendering is complete:
//   basic → `"…"` for s/o/g, `true`/`false` for b, bare numbers otherwise;
//   v     → `<value>` (type inferred at this position);
//   a<X>  → `[e0,e1,…]`;
//   a{KV} → `{k0:v0, k1:v1, …}`;
//   (…)   → `(f0,f1,…)`.
//
// GVariant text strings may be `"…"` or `'…'` (equivalent per the GLib
// GVariant text-format spec); we always emit `"…"` for values and keys alike.

fn gdbus_value(ty: &str, tok: &mut Tok<'_>) -> Rendered {
    // Basic type.
    match ty {
        "s" | "o" | "g" => return Rendered::ok(gdbus_string(tok.next())),
        "b" => return Rendered::ok(gdbus_bool(tok.next())),
        "y" | "n" | "q" | "i" | "u" | "x" | "t" | "d" => return Rendered::ok(tok.next().into()),
        _ => {}
    }
    match ty {
        "v" => {
            // The inner-signature token tells us the variant's type; render the
            // value as that type wrapped in `<…>` (GVariant variant literal).
            let inner = tok.next();
            let inner_val = gdbus_value(inner, tok).text;
            Rendered::ok(format!("<{inner_val}>"))
        }
        t if t.starts_with('a') && !t.starts_with("a{") => {
            let elem = &t[1..];
            let n = tok.next_count();
            let mut parts: Vec<String> = Vec::with_capacity(n);
            for _ in 0..n {
                let r = gdbus_value(elem, tok);
                parts.push(r.text);
            }
            Rendered::ok(format!("[{}]", parts.join(",")))
        }
        t if t.starts_with("a{") => {
            let inner = &t["a{".len()..t.len() - 1];
            let chars: Vec<char> = inner.chars().collect();
            let mut pos = 0;
            if skip_complete_type(&chars, &mut pos).is_err() {
                return Rendered::unsupported(t.to_string());
            }
            let key_sig: String = chars[..pos].iter().collect();
            let vstart = pos;
            if skip_complete_type(&chars, &mut pos).is_err() {
                return Rendered::unsupported(t.to_string());
            }
            let val_sig: String = chars[vstart..pos].iter().collect();
            let n = tok.next_count();
            let mut parts: Vec<String> = Vec::with_capacity(n);
            for _ in 0..n {
                let k = gdbus_value(&key_sig, tok).text;
                let v = gdbus_value(&val_sig, tok).text;
                parts.push(format!("{k}:{v}"));
            }
            Rendered::ok(format!("{{{}}}", parts.join(",")))
        }
        t if t.starts_with('(') => {
            let inner = &t[1..t.len() - 1];
            let chars: Vec<char> = inner.chars().collect();
            let mut pos = 0;
            let mut parts: Vec<String> = Vec::new();
            while pos < chars.len() {
                let start = pos;
                if skip_complete_type(&chars, &mut pos).is_err() {
                    break;
                }
                let field: String = chars[start..pos].iter().collect();
                parts.push(gdbus_value(&field, tok).text);
            }
            Rendered::ok(format!("({})", parts.join(",")))
        }
        _ => Rendered::ok(gdbus_string(tok.next())),
    }
}

// --- helpers ---------------------------------------------------------------

/// Shell-quote a token if it needs it (whitespace or special chars), wrapping
/// in `"..."` and escaping `\` and `"`. Tokens that need no quoting pass
/// through unchanged.
fn quote(s: &str) -> String {
    let needs = s.is_empty()
        || s.chars().any(|c| {
            c.is_whitespace()
                || matches!(
                    c,
                    '"' | '\\'
                        | '\''
                        | '$'
                        | '`'
                        | '<'
                        | '>'
                        | '&'
                        | ';'
                        | '|'
                        | '*'
                        | '?'
                        | '('
                        | ')'
                )
        });
    if !needs {
        return s.into();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Split a D-Bus signature into its top-level complete types
/// (e.g. `"su"` → `["s","u"]`, `"as"` → `["as"]`, `"(ii)s"` → `["(ii)","s"]`).
///
/// A complete type is: one basic code, an array `a…` (one complete element
/// type), a dict `a{KV}` (two complete types), or a struct `(...)` (complete
/// types until `)`). Mirrors the walker in [`crate::value::encode`].
fn split_signature(sig: &str) -> Vec<String> {
    let chars: Vec<char> = sig.chars().collect();
    let mut pos = 0;
    let mut out = Vec::new();
    while pos < chars.len() {
        let start = pos;
        if let Err(()) = skip_complete_type(&chars, &mut pos) {
            break;
        }
        out.push(chars[start..pos].iter().collect());
    }
    out
}

/// Advance `pos` past one complete type, or return `Err(())` on truncation.
fn skip_complete_type(chars: &[char], pos: &mut usize) -> Result<(), ()> {
    match chars.get(*pos).copied() {
        None => Err(()),
        // basic types + variant
        Some('y' | 'b' | 'n' | 'q' | 'i' | 'u' | 'x' | 't' | 'd' | 's' | 'o' | 'g' | 'v') => {
            *pos += 1;
            Ok(())
        }
        Some('a') => {
            *pos += 1;
            match chars.get(*pos).copied() {
                Some('{') => {
                    *pos += 1; // consume '{'
                    skip_complete_type(chars, pos)?; // key
                    skip_complete_type(chars, pos)?; // value
                    match chars.get(*pos).copied() {
                        Some('}') => {
                            *pos += 1;
                            Ok(())
                        }
                        _ => Err(()),
                    }
                }
                _ => skip_complete_type(chars, pos), // element type
            }
        }
        Some('(') => {
            *pos += 1; // consume '('
            while matches!(chars.get(*pos).copied(), Some(c) if c != ')') {
                skip_complete_type(chars, pos)?;
            }
            match chars.get(*pos).copied() {
                Some(')') => {
                    *pos += 1;
                    Ok(())
                }
                _ => Err(()),
            }
        }
        // Unknown type code — consume it so the caller can keep walking;
        // the result is best-effort anyway.
        Some(_) => {
            *pos += 1;
            Ok(())
        }
    }
}

/// Whether a basic type code maps to a qdbus "literal" value (used to decide
/// basic-vs-array handling). Returns `Some(())` for basic codes.
fn dbus_send_basic_literal_kind(ty: &str) -> Option<()> {
    matches!(
        ty,
        "y" | "b" | "n" | "q" | "i" | "u" | "x" | "t" | "d" | "s" | "o" | "g"
    )
    .then_some(())
}

/// GVariant-text double-quoted string (escapes `\` and `"`).
fn gdbus_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// GVariant-text boolean (qdbus/busctl accept `true`/`false`/`1`/`0`).
fn gdbus_bool(s: &str) -> String {
    match s {
        "true" | "yes" | "on" | "1" => "true".into(),
        _ => "false".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_signature_basics() {
        assert_eq!(split_signature("su"), vec!["s", "u"]);
        assert_eq!(split_signature("as"), vec!["as"]);
        assert_eq!(split_signature("a{sv}"), vec!["a{sv}"]);
        assert_eq!(split_signature("(ii)s"), vec!["(ii)", "s"]);
        assert_eq!(split_signature("a(ii)u"), vec!["a(ii)", "u"]);
        assert_eq!(split_signature(""), Vec::<String>::new());
    }

    #[test]
    fn quote_only_when_needed() {
        assert_eq!(quote("42"), "42");
        assert_eq!(quote("hi"), "hi");
        assert_eq!(quote("a b"), "\"a b\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote(""), "\"\"");
        assert_eq!(quote("type='signal'"), "\"type='signal'\"");
    }
}
