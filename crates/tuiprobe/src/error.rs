// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// // SPDX-License-Identifier: MIT

use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Pty(String),
    Io(std::io::Error),
    Timeout { what: String, screen: String },
    ProcessExited,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Pty(msg) => write!(f, "pty error: {msg}"),
            Error::Io(e) => write!(f, "io error: {e}"),
            Error::Timeout { what, screen } => {
                write!(f, "timeout waiting for: {what}\n\ncurrent screen:\n{screen}")
            }
            Error::ProcessExited => write!(f, "child process exited unexpectedly"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}
