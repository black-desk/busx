// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Resolve a [`zvariant::Value::Fd`] into something human-meaningful.
//!
//! A D-Bus `h` (UNIX_FD) is just an index into an out-of-band fd list; the fd
//! itself is a real, duplicated file descriptor in *this* process (received via
//! `SCM_RIGHTS`). That lets us describe it — but only for as long as the fd is
//! alive, i.e. while the carrying message is borrowed. So [`gather`] runs
//! synchronously during rendering and the result is plain data (no fd held):
//! the message is then free to drop and close the fd.
//!
//! That lifetime rule is not cosmetic. A monitor holding a duplicated pipe
//! read-end keeps the writer from observing the close (EPIPE) until *every*
//! copy — including the monitor's — is released. busx renders to a string at
//! receive-time and never buffers the live fd, so it releases its copy within a
//! single frame.
//!
//! Sources (all Linux `/proc` + `fstat`, matching the rest of busx — no new
//! deps):
//! - `fstat(2)` → file type (regular/dir/socket/fifo/char/block) + size.
//! - `readlink /proc/self/fd/<n>` → the target (path, `pipe:[ino]`,
//!   `socket:[ino]`, `anon_inode:[eventfd]`, `/memfd:NAME (deleted)`, …).
//! - `/proc/self/fdinfo/<n>` → `flags:` (access mode: ro/wo/rw) and, for a few
//!   anon-inode types, one useful piece of state (`eventfd-count`).

use std::fs;

/// A resolved view of a unix fd. Pure data — building it touches `/proc` and
/// `fstat` but stores no handle, so it outlives the fd itself.
pub(crate) struct FdInfo {
    /// Short kind label: `regular`, `directory`, `socket`, `pipe`, `char`,
    /// `block`, the `anon_inode` inner name (`eventfd`, `timerfd`, …), or
    /// `unknown`.
    pub kind: String,
    /// The raw `readlink` target, when `/proc/self/fd/<n>` was readable.
    pub target: Option<String>,
    /// Access mode from fdinfo `flags`: `ro`, `wo`, `rw`, or `""` if unknown.
    pub mode: &'static str,
    /// File size, for regular files only.
    pub size: Option<u64>,
    /// One type-specific detail from fdinfo when it adds real signal
    /// (currently `count=<n>` for `eventfd`).
    pub note: Option<String>,
}

/// Inspect `raw` and return a plain-data description. Never panics: any
/// `/proc`/`fstat` failure degrades to `unknown` / `None` so rendering still
/// produces *something* rather than aborting output.
pub(crate) fn gather(raw: i32) -> FdInfo {
    let target = fs::read_link(format!("/proc/self/fd/{raw}"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned());

    let (mut kind, size) = fstat_kind_size(raw);
    // anon-inode fds (eventfd/timerfd/signalfd/…) and memfd carry their real
    // identity in the readlink target, not in fstat's mode bits — prefer that.
    if let Some(t) = &target {
        if let Some(inner) = t
            .strip_prefix("anon_inode:[")
            .and_then(|s| s.strip_suffix(']'))
        {
            kind = inner.to_string();
        } else if t.starts_with("/memfd:") {
            kind = "memfd".into();
        }
    }

    let fdinfo = fs::read_to_string(format!("/proc/self/fdinfo/{raw}")).ok();
    let mode = fdinfo.as_ref().and_then(|s| flags_mode(s)).unwrap_or("");
    let note = fdinfo.as_ref().and_then(|s| eventfd_note(s, &kind));

    FdInfo {
        kind,
        target,
        mode,
        size,
        note,
    }
}

/// Human-friendly label for the pretty-printer: the `anon_inode:[x]` target
/// collapses to its inner name; everything else uses the target verbatim
/// (paths, `pipe:[ino]`, `socket:[ino]`, `/memfd:…`).
pub(crate) fn pretty_label(info: &FdInfo) -> String {
    if let Some(t) = &info.target {
        if let Some(inner) = t
            .strip_prefix("anon_inode:[")
            .and_then(|s| s.strip_suffix(']'))
        {
            return inner.to_string();
        }
        return t.clone();
    }
    // No /proc target at all — fall back to the kind, then to "fd".
    if info.kind != "unknown" {
        info.kind.clone()
    } else {
        "fd".into()
    }
}

/// `(kind, size)` from `fstat`; `(unknown, None)` on failure.
fn fstat_kind_size(raw: i32) -> (String, Option<u64>) {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(raw, &mut st) } != 0 {
        return ("unknown".into(), None);
    }
    // `st_mode` and the S_IF* constants are both mode_t (u32 on Linux).
    let mode = st.st_mode as u32;
    let fmt = mode & libc::S_IFMT;
    let kind = match fmt {
        libc::S_IFREG => "regular",
        libc::S_IFDIR => "directory",
        libc::S_IFSOCK => "socket",
        libc::S_IFIFO => "pipe",
        libc::S_IFCHR => "char",
        libc::S_IFBLK => "block",
        libc::S_IFLNK => "symlink",
        _ => "unknown",
    };
    let size = (fmt == libc::S_IFREG).then_some(st.st_size as u64);
    (kind.into(), size)
}

/// Parse fdinfo `flags:` (octal) into an access-mode label.
fn flags_mode(fdinfo: &str) -> Option<&'static str> {
    for line in fdinfo.lines() {
        if let Some(rest) = line.strip_prefix("flags:") {
            let flags = u32::from_str_radix(rest.trim(), 8).unwrap_or(0);
            return Some(match flags & libc::O_ACCMODE as u32 {
                0 => "ro",
                1 => "wo",
                2 => "rw",
                _ => "",
            });
        }
    }
    None
}

/// For `eventfd`, surface `eventfd-count` (the one genuinely useful piece of
/// state). Other anon types could grow similar arms; kept minimal for now.
fn eventfd_note(fdinfo: &str, kind: &str) -> Option<String> {
    if kind != "eventfd" {
        return None;
    }
    for line in fdinfo.lines() {
        if let Some(rest) = line.strip_prefix("eventfd-count:") {
            return Some(format!("count={}", rest.trim()));
        }
    }
    None
}
