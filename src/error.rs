// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::convert::Infallible;
use std::process::ExitCode;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")]
    Zbus(#[from] zbus::Error),
    #[error("{0}")]
    Fdo(#[from] zbus::fdo::Error),
    #[error("{0}")]
    Zvariant(#[from] zvariant::Error),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Msg(String),
    // Never occurs in practice (TestBackend's draw is infallible); exists so the
    // generic `App::run_loop` can lift `<B as Backend>::Error` into our `Error`.
    #[error("infallible")]
    Infallible(#[from] Infallible),
}

impl Error {
    /// All failures exit 1 (spec §9).
    pub fn exit_code(&self) -> ExitCode {
        ExitCode::FAILURE
    }
}
