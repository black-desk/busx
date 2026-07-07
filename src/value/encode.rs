// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `busx` value encoder — busctl-style signature + positional tokens →
//! [`zvariant::Value`] (spec §7.1).
//!
//! The **signature string** is passed separately; the value tokens are laid out
//! positionally per busctl rules:
//!
//! - basic type → one value token (`b` accepts `true`/`yes`/`on`/`1` and
//!   `false`/`no`/`off`/`0`, case-sensitive per busctl).
//! - `v` (variant) → next token is the inner signature, then its value.
//! - `a<X>` (array) → next token is the element count `N`, then `N` elements.
//! - `a{KV}` (dict array) → count `N`, then `N` pairs (key then value).
//! - `(...)` (struct) → fields laid out flat until `)`.
//!
//! Nesting is fully supported (`av`, `a{sv}`, `a(as)`, empty `as 0`, ...) —
//! unlike `dbus-send`.
//!
//! Design note: rather than reconstructing [`zvariant::Signature`] nodes by
//! hand, the parser pulls **one complete type** out of the signature stream as a
//! substring and validates it through [`Signature::try_from`]. That yields the
//! canonical [`Signature`] values that `Array::new` / `Dict::new` need (and that
//! their `append` checks compare against), with no risk of hand-built
//! mismatches.

use crate::error::{Error, Result};
use zvariant::{Array, Dict, ObjectPath, Signature, StructureBuilder, Value};

/// Cursor over the positional value tokens.
struct Cur<'a> {
    toks: &'a [String],
    pos: usize,
}

impl<'a> Cur<'a> {
    fn next(&mut self) -> Result<&'a str> {
        let t = self
            .toks
            .get(self.pos)
            .ok_or_else(|| Error::Msg("not enough arguments".into()))?;
        self.pos += 1;
        Ok(t.as_str())
    }

    /// Read a count token (busctl arrays/dicts prefix their elements with `N`).
    fn next_count(&mut self) -> Result<usize> {
        let t = self.next()?;
        t.parse::<usize>()
            .map_err(|e| Error::Msg(format!("invalid element count `{t}`: {e}")))
    }

    fn remaining(&self) -> usize {
        self.toks.len().saturating_sub(self.pos)
    }
}

/// Parse busctl-style input: `signature` is the type code string and `values`
/// are the positional value tokens laid out per busctl rules.
pub fn parse(signature: &str, values: &[String]) -> Result<Vec<Value<'static>>> {
    let sig: Vec<char> = signature.chars().collect();
    let mut cur = Cur { toks: values, pos: 0 };
    let mut st = SigStream { chars: &sig, pos: 0 };

    let mut out = Vec::new();
    while !st.done() {
        out.push(parse_type(&mut st, &mut cur)?);
    }
    if cur.remaining() != 0 {
        return Err(Error::Msg(format!(
            "{} extra argument(s)",
            cur.remaining()
        )));
    }
    Ok(out)
}

/// Index-based cursor over the signature characters. Indexing (rather than a
/// `Peekable<Chars>`) lets us slice out complete-type substrings for validation.
struct SigStream<'a> {
    chars: &'a [char],
    pos: usize,
}

impl<'a> SigStream<'a> {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }

    fn done(&self) -> bool {
        self.pos >= self.chars.len()
    }

    fn expect(&mut self, c: char) -> Result<()> {
        match self.next() {
            Some(got) if got == c => Ok(()),
            Some(got) => Err(Error::Msg(format!(
                "invalid signature: expected `{c}`, got `{got}`"
            ))),
            None => Err(Error::Msg(format!(
                "invalid signature: expected `{c}`, end of signature"
            ))),
        }
    }
}

/// Parse one complete type out of the signature, consuming its value tokens.
fn parse_type(st: &mut SigStream<'_>, cur: &mut Cur<'_>) -> Result<Value<'static>> {
    let c = st.next().ok_or_else(|| Error::Msg("truncated signature".into()))?;
    Ok(match c {
        // --- basic types ---
        'y' => Value::U8(parse_num(cur.next()?, "byte")?),
        'b' => Value::Bool(parse_bool(cur.next()?)),
        'n' => Value::I16(parse_num(cur.next()?, "int16")?),
        'q' => Value::U16(parse_num(cur.next()?, "uint16")?),
        'i' => Value::I32(parse_num(cur.next()?, "int32")?),
        'u' => Value::U32(parse_num(cur.next()?, "uint32")?),
        'x' => Value::I64(parse_num(cur.next()?, "int64")?),
        't' => Value::U64(parse_num(cur.next()?, "uint64")?),
        'd' => Value::F64(parse_num(cur.next()?, "double")?),
        's' => Value::Str(cur.next()?.to_string().into()),
        // Take ownership so the resulting `ObjectPath<'static>` is `'static`.
        'o' => Value::ObjectPath(ObjectPath::try_from(cur.next()?.to_string())?),
        'g' => {
            // A signature value is itself a valid signature string.
            let s = cur.next()?;
            let sig = parse_signature(s)?;
            Value::Signature(sig)
        }

        // --- variant: next token is the inner signature, then its value ---
        'v' => {
            let inner_sig = cur.next()?;
            // Validate the inner signature and re-derive as a char slice we own.
            parse_signature(inner_sig)?;
            let inner: Vec<char> = inner_sig.chars().collect();
            let mut ist = SigStream { chars: &inner, pos: 0 };
            let val = parse_type(&mut ist, cur)?;
            if !ist.done() {
                return Err(Error::Msg(format!(
                    "variant signature `{inner_sig}` has more than one type"
                )));
            }
            Value::Value(Box::new(val))
        }

        // --- array / dict array ---
        'a' => match st.peek() {
            Some('{') => {
                st.next();
                parse_dict(st, cur)?
            }
            _ => {
                // The element type is one complete type following `a`. Capture
                // its signature substring (for an empty array we still need it
                // to build a typed `Array`), then parse `N` elements.
                let elem_start = st.pos;
                skip_complete_type(st)?;
                let elem_sig: String = st.chars[elem_start..st.pos].iter().collect();
                parse_array(&elem_sig, cur)?
            }
        },

        // --- struct: `(` already consumed; fields follow until `)`.
        '(' => parse_struct(st, cur)?,

        other => {
            return Err(Error::Msg(format!(
                "unsupported type code `{other}`"
            )))
        }
    })
}

/// `a{KV}`: dict. `{` already consumed; we are at the key type.
fn parse_dict(st: &mut SigStream<'_>, cur: &mut Cur<'_>) -> Result<Value<'static>> {
    // Record the signature char ranges of the key and value types so each entry
    // can be re-parsed independently.
    let key_start = st.pos;
    skip_complete_type(st)?;
    let key_end = st.pos;
    let val_start = st.pos;
    skip_complete_type(st)?;
    let val_end = st.pos;
    st.expect('}')?;
    let key_sig = substring_sig(st, key_start, key_end)?;
    let val_sig = substring_sig(st, val_start, val_end)?;

    let n = cur.next_count()?;
    let mut dict = Dict::new(&key_sig, &val_sig);
    for _ in 0..n {
        let key = parse_type_at(st, key_start, key_end, cur)?;
        let val = parse_type_at(st, val_start, val_end, cur)?;
        dict.append(key, val)?;
    }
    Ok(Value::Dict(dict))
}

/// `a<X>`: homogenous array. `elem_sig` is the validated element signature; the
/// element type has already been consumed from `st`.
fn parse_array(elem_sig: &str, cur: &mut Cur<'_>) -> Result<Value<'static>> {
    let n = cur.next_count()?;
    // Validate once; keep the owned `Signature` alive for the whole loop
    // (Array::new borrows it, and Array::append checks against it).
    let elem_signature = parse_signature(elem_sig)?;
    let mut arr = Array::new(&elem_signature);
    let elem_sig_str = elem_sig.to_string();
    for _ in 0..n {
        let v = parse_sig_type(&elem_sig_str, cur)?;
        arr.append(v)?;
    }
    Ok(Value::Array(arr))
}

/// Parse one complete type from a standalone signature string, consuming its
/// value tokens. Used to re-parse repeated array elements (whose signature was
/// already extracted from the outer stream).
fn parse_sig_type(sig: &str, cur: &mut Cur<'_>) -> Result<Value<'static>> {
    let chars: Vec<char> = sig.chars().collect();
    let mut st = SigStream { chars: &chars, pos: 0 };
    let v = parse_type(&mut st, cur)?;
    if !st.done() {
        return Err(Error::Msg(format!(
            "signature `{sig}` contains more than one type"
        )));
    }
    Ok(v)
}

/// `(...)`: struct. `(` already consumed; fields follow.
fn parse_struct(st: &mut SigStream<'_>, cur: &mut Cur<'_>) -> Result<Value<'static>> {
    let mut builder = StructureBuilder::new();
    loop {
        match st.peek() {
            Some(')') => {
                st.next();
                break;
            }
            None => return Err(Error::Msg("unterminated struct in signature".into())),
            Some(_) => {
                let field = parse_type(st, cur)?;
                builder = builder.append_field(field);
            }
        }
    }
    Ok(Value::Structure(builder.build()?))
}

/// Re-parse one complete type whose signature occupies `st[start..end)`,
/// consuming its value tokens. Used by the dict parser, which must re-enter the
/// parser at a saved signature range (the key / value type) for each entry.
fn parse_type_at(
    st: &SigStream<'_>,
    start: usize,
    end: usize,
    cur: &mut Cur<'_>,
) -> Result<Value<'static>> {
    let sub: Vec<char> = st.chars[start..end].to_vec();
    parse_sig_type(&sub.iter().collect::<String>(), cur)
}

/// Advance `st` past one complete type without producing a value. Handles
/// `a{...}`, `a(...)` / `(...)`, and `a`+element recursively, plus all basic
/// codes and `v`.
fn skip_complete_type(st: &mut SigStream<'_>) -> Result<()> {
    match st.next() {
        None => Err(Error::Msg("truncated signature".into())),
        Some('y' | 'b' | 'n' | 'q' | 'i' | 'u' | 'x' | 't' | 'd' | 's' | 'o' | 'g' | 'v') => Ok(()),
        Some('a') => match st.peek() {
            Some('{') => {
                st.next();
                skip_complete_type(st)?; // key
                skip_complete_type(st)?; // value
                st.expect('}')
            }
            _ => skip_complete_type(st), // element type
        },
        Some('(') => {
            while matches!(st.peek(), Some(c) if c != ')') {
                skip_complete_type(st)?;
            }
            st.expect(')')
        }
        Some(other) => Err(Error::Msg(format!("unsupported type code `{other}`"))),
    }
}

/// Validate a signature string, returning the canonical [`Signature`]. The
/// `Signature::try_from` error type (`zvariant::signature::Error`) is bridged to
/// [`zvariant::Error`] (and hence our [`Error`]) via zvariant's own `From` impl.
fn parse_signature(s: &str) -> Result<Signature> {
    Signature::try_from(s).map_err(zvariant::Error::from).map_err(Error::from)
}

/// Collect `st[start..end)` into a validated [`Signature`] (the key / value type
/// of a dict).
fn substring_sig(st: &SigStream<'_>, start: usize, end: usize) -> Result<Signature> {
    let s: String = st.chars[start..end].iter().collect();
    parse_signature(&s)
}

fn parse_bool(s: &str) -> bool {
    matches!(s, "true" | "yes" | "on" | "1")
}

fn parse_num<T>(s: &str, what: &str) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    s.parse::<T>()
        .map_err(|e| Error::Msg(format!("invalid {what} `{s}`: {e}")))
}
