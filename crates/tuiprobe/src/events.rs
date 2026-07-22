// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: MIT

//! Keyboard / mouse event types and their encoding to terminal escape
//! sequences.
//!
//! Key encodings are based on what crossterm expects in raw mode (the most
//! common backend for ratatui apps). The critical detail: **Enter is `\r`
//! (CR), not `\n` (LF)**. In raw mode crossterm only maps `\r` → `KeyCode::Enter`
//! — `\n` falls through to `Ctrl+J`.

/// A keyboard key, mirroring crossterm's `KeyCode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCode {
    Char(char),
    Enter,
    Tab,
    Backspace,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Insert,
    F(u8),
}

/// Modifier flags for key / mouse events.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyModifiers {
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
    };
    pub const CTRL: Self = Self {
        ctrl: true,
        alt: false,
        shift: false,
    };
    pub const ALT: Self = Self {
        ctrl: false,
        alt: true,
        shift: false,
    };
    pub const SHIFT: Self = Self {
        ctrl: false,
        alt: false,
        shift: true,
    };
}

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Scroll wheel direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollDirection {
    Up,
    Down,
}

/// Encode a key + modifiers into the byte sequence a terminal expects.
///
/// These bytes are written to the PTY *master*; the child process reads them
/// from its stdin (the PTY *slave*). crossterm / termion / termwiz running
/// inside the child will decode them back into key events.
pub fn encode_key(key: KeyCode, mods: KeyModifiers) -> Vec<u8> {
    // Ctrl + Char → ASCII control character (0x00–0x1F).
    if mods.ctrl
        && let KeyCode::Char(c) = key
    {
        return encode_ctrl(c);
    }

    // Alt + Char → ESC prefix.
    if mods.alt
        && let KeyCode::Char(c) = key
    {
        let mut bytes = vec![0x1b];
        bytes.extend_from_slice(c.to_string().as_bytes());
        return bytes;
    }

    match key {
        KeyCode::Char(c) => c.to_string().into_bytes(),
        // CR (0x0D) — crossterm in raw mode maps \r to KeyCode::Enter.
        // \n (0x0A) is only recognized as Enter when raw mode is OFF.
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::Esc => vec![0x1b],
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::F(n) => encode_function_key(n),
    }
}

fn encode_ctrl(c: char) -> Vec<u8> {
    let c = c.to_ascii_lowercase();
    let byte = match c {
        'a'..='z' => (c as u8) - b'a' + 1,
        '@' => 0,
        '[' => 0x1b,
        '\\' => 0x1c,
        ']' => 0x1d,
        '^' => 0x1e,
        '_' => 0x1f,
        '?' => 0x7f,
        _ => return c.to_string().into_bytes(),
    };
    vec![byte]
}

fn encode_function_key(n: u8) -> Vec<u8> {
    match n {
        1 => b"\x1bOP".to_vec(),
        2 => b"\x1bOQ".to_vec(),
        3 => b"\x1bOR".to_vec(),
        4 => b"\x1bOS".to_vec(),
        5 => b"\x1b[15~".to_vec(),
        6 => b"\x1b[17~".to_vec(),
        7 => b"\x1b[18~".to_vec(),
        8 => b"\x1b[19~".to_vec(),
        9 => b"\x1b[20~".to_vec(),
        10 => b"\x1b[21~".to_vec(),
        11 => b"\x1b[23~".to_vec(),
        12 => b"\x1b[24~".to_vec(),
        _ => Vec::new(),
    }
}

/// Encode a mouse button press/release as an SGR mouse sequence.
///
/// Format: `ESC [ < button ; col ; row M` (press) or `m` (release).
/// Coordinates are 1-based. Requires the child to have SGR mouse mode
/// enabled (e.g. crossterm's `EnableMouseCapture`).
pub fn encode_mouse(col: u16, row: u16, button: MouseButton, pressed: bool) -> Vec<u8> {
    let btn_code = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    let suffix = if pressed { 'M' } else { 'm' };
    format!("\x1b[<{btn_code};{};{}{suffix}", col + 1, row + 1).into_bytes()
}

/// Encode a mouse scroll event as an SGR mouse sequence.
pub fn encode_scroll(col: u16, row: u16, dir: ScrollDirection) -> Vec<u8> {
    let btn_code = match dir {
        ScrollDirection::Up => 64,
        ScrollDirection::Down => 65,
    };
    format!("\x1b[<{btn_code};{};{}M", col + 1, row + 1).into_bytes()
}
