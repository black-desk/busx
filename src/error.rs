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
}

impl Error {
    /// All failures exit 1 (spec §9).
    pub fn exit_code(&self) -> ExitCode {
        ExitCode::FAILURE
    }
}
