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
    ZbusXml(#[from] zbus_xml::Error),
    /// A typed error wrapped with human context; the original cause is kept as
    /// the source so `-v` can walk the full cause chain (`Error::source`).
    #[error("{context}")]
    Context {
        context: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
    #[error("{0}")]
    Msg(String),
}

// `TestBackend`'s draw is infallible; this lets the generic `App::run_loop` lift
// `<B as Backend>::Error` into `Error` without a dead enum variant.
impl From<Infallible> for Error {
    fn from(i: Infallible) -> Self {
        match i {}
    }
}

impl Error {
    /// Wrap a typed error with human `context`, preserving the original as the
    /// `source` so `-v` can print the full cause chain.
    pub fn context(
        context: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Error::Context {
            context: context.into(),
            source: Box::new(source),
        }
    }

    /// All failures exit 1.
    pub fn exit_code(&self) -> ExitCode {
        ExitCode::FAILURE
    }
}
