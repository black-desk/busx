// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Human-friendly rendering of a [`zvariant::Value`].
//!
//! This is the value pretty-printer used by every command's *default* output
//! (human text). It mirrors the variant walk in [`crate::value::decode`] so the
//! accessor patterns stay in lock-step, but renders a compact, shell-friendly
//! form instead of type-tagged JSON:
//!
//! - numbers (U8/I16/U32/U64/I64/F64, plus U16/I32) → bare number.
//! - Bool → `true`/`false`.
//! - Str / ObjectPath / Signature → a Rust-style quoted literal `"..."` via
//!   `{:?}`, so control characters are escaped (`\n`, `\t`, `\u{1}`, …) and
//!   don't break the line-based output.
//! - Variant → `<{inner_type_sig} {pretty(inner)}>` (e.g. `<d 0.5>`).
//! - Array → `[e0, e1, ...]`; a byte array (`ay`) → `b"..."` (printable ASCII
//!   verbatim, else `\xNN`).
//! - Dict → `{k0: v0, k1: v1, ...}` (works for every key type — the human form
//!   has no JSON object-key limit, so even `a{uu}` renders as `{1:10, 2:20}`).
//! - Structure → `(f0, f1, ...)`.
//! - Fd → `<fd TARGET [SIZE] [MODE] [NOTE]>` (resolved via `/proc` + `fstat`).
//! - anything else (Maybe under a gated feature) → `unsupported value: …`.

use std::os::fd::AsRawFd;
use zvariant::Value;

/// Render a borrowed [`Value`] as a human-friendly string.
pub fn pretty(v: &Value<'_>) -> String {
    match v {
        Value::U8(b) => b.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::I16(i) => i.to_string(),
        Value::U16(i) => i.to_string(),
        Value::I32(i) => i.to_string(),
        Value::U32(i) => i.to_string(),
        Value::I64(i) => i.to_string(),
        Value::U64(i) => i.to_string(),
        Value::F64(d) => d.to_string(),
        Value::Str(s) => format!("{:?}", s.as_str()),
        Value::Signature(s) => format!("{:?}", s.to_string()),
        Value::ObjectPath(o) => format!("{:?}", o.as_str()),

        // A variant carries its own inner signature, so render it tag-first.
        // `value_signature()` gives the enclosed type (e.g. `d`), whereas the
        // trait `DynamicType::signature` would always return `v`.
        Value::Value(inner) => format!("<{} {}>", inner.value_signature(), pretty(inner)),
        Value::Array(a) => {
            // A byte array (`ay`) renders as a Rust-style bytestring
            // (`b"..."`): printable ASCII verbatim, the rest `\xNN`. Other
            // arrays stay `[e0, e1, ...]`.
            if a.element_signature().to_string() == "y" {
                let bytes: Vec<u8> = a
                    .inner()
                    .iter()
                    .filter_map(|v| match v {
                        Value::U8(b) => Some(*b),
                        _ => None,
                    })
                    .collect();
                format_bytestring(&bytes)
            } else {
                bracket('[', ']', a.inner().iter().map(pretty))
            }
        }
        Value::Structure(s) => bracket('(', ')', s.fields().iter().map(pretty)),
        Value::Dict(d) => bracket(
            '{',
            '}',
            d.iter()
                .map(|(k, v)| format!("{}: {}", pretty(k), pretty(v))),
        ),
        // Real fd, duplicated into this process via SCM_RIGHTS. Resolve it to a
        // label/kind/mode via /proc + fstat (see `fdinfo`). Never hold the fd.
        Value::Fd(fd) => format_fd(&crate::value::fdinfo::gather(fd.as_raw_fd())),
        // Optional/platform variant: Maybe (gvariant feature, not enabled).
        // Render a clear placeholder rather than crashing on an unfamiliar type.
        #[allow(unreachable_patterns)]
        other => format!("unsupported value: {other:?}"),
    }
}

/// Expand the compact human representation for viewing in an external editor.
///
/// The compact representation is deliberately kept for the terminal and CLI.
/// This pass only changes the layout: delimiters and commas outside quoted
/// strings become line breaks, with two spaces per nesting level. Quoted values
/// are copied byte-for-byte, so punctuation in a string is left untouched.
pub fn format_for_editor(text: &str) -> String {
    let mut output = String::with_capacity(text.len() + text.len() / 4);
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    let mut fd_literal = false;
    let mut line_start = true;

    for (index, ch) in text.char_indices() {
        if fd_literal {
            output.push(ch);
            if ch == '>' {
                fd_literal = false;
            }
            line_start = false;
            continue;
        }

        if quoted {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                quoted = false;
            }
            line_start = false;
            continue;
        }

        if ch == '"' {
            quoted = true;
            output.push(ch);
            line_start = false;
            continue;
        }

        if text[index..].starts_with("<fd ") {
            fd_literal = true;
            output.push(ch);
            line_start = false;
            continue;
        }

        match ch {
            '[' | '{' | '(' => {
                output.push(ch);
                depth += 1;
                if next_non_whitespace(text, index + ch.len_utf8()) != Some(close_for(ch)) {
                    push_editor_newline(&mut output, depth);
                    line_start = true;
                }
            }
            ']' | '}' | ')' => {
                depth = depth.saturating_sub(1);
                let empty = output
                    .trim_end_matches([' ', '\t'])
                    .ends_with(['[', '{', '(']);
                if !line_start && !empty {
                    push_editor_newline(&mut output, depth);
                } else {
                    trim_editor_line(&mut output);
                    push_editor_indent(&mut output, depth);
                }
                output.push(ch);
                line_start = false;
            }
            ',' => {
                output.push(ch);
                if depth > 0 {
                    push_editor_newline(&mut output, depth);
                    line_start = true;
                }
            }
            '\n' => {
                trim_editor_line(&mut output);
                output.push('\n');
                line_start = true;
            }
            ch if ch.is_whitespace() && line_start => {}
            _ => {
                output.push(ch);
                line_start = false;
            }
        }
    }
    output
}

fn close_for(open: char) -> char {
    match open {
        '[' => ']',
        '{' => '}',
        '(' => ')',
        _ => unreachable!(),
    }
}

fn next_non_whitespace(text: &str, start: usize) -> Option<char> {
    text[start..].chars().find(|ch| !ch.is_whitespace())
}

fn trim_editor_line(output: &mut String) {
    while output.ends_with([' ', '\t']) {
        output.pop();
    }
}

fn push_editor_indent(output: &mut String, depth: usize) {
    output.extend(std::iter::repeat_n(' ', depth * 2));
}

fn push_editor_newline(output: &mut String, depth: usize) {
    trim_editor_line(output);
    output.push('\n');
    push_editor_indent(output, depth);
}

/// Render an fd as `<fd TARGET [SIZE] [MODE] [NOTE]>`. The target is resolved
/// from `/proc/self/fd/<n>`; size is shown for regular files; mode (ro/wo/rw)
/// and the eventfd count come from fdinfo. Falls back to `<fd unknown>` if
/// `/proc` is unavailable. See [`crate::value::fdinfo`].
fn format_fd(info: &crate::value::fdinfo::FdInfo) -> String {
    let label = crate::value::fdinfo::pretty_label(info);
    let mut s = format!("<fd {label}");
    if let Some(size) = info.size {
        s.push_str(&format!(" {size}B"));
    }
    if !info.mode.is_empty() {
        s.push_str(&format!(" {}", info.mode));
    }
    if let Some(note) = &info.note {
        s.push_str(&format!(" {note}"));
    }
    s.push('>');
    s
}

/// Render a byte array (`ay`) as a Rust-style bytestring `b"..."`: printable
/// ASCII (0x20–0x7e) is shown verbatim, every other byte as lowercase `\xNN`,
/// and `"` / `\` are escaped — mirroring Rust's own byte-string literal syntax.
fn format_bytestring(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() + 3);
    s.push_str("b\"");
    for &b in bytes {
        match b {
            b'"' => s.push_str("\\\""),
            b'\\' => s.push_str("\\\\"),
            0x20..=0x7e => s.push(b as char),
            _ => s.push_str(&format!("\\x{b:02x}")),
        }
    }
    s.push('"');
    s
}

/// Join an iterator of already-rendered element strings between `open`/`close`.
fn bracket<I>(open: char, close: char, items: I) -> String
where
    I: IntoIterator<Item = String>,
{
    let mut s = String::new();
    s.push(open);
    let mut first = true;
    for it in items {
        if !first {
            s.push_str(", ");
        }
        first = false;
        s.push_str(&it);
    }
    s.push(close);
    s
}
