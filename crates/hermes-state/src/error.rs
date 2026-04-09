//! Crate-level error type for hermes-state.

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0}")]
    Message(String),

    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
}

impl hermes_error::FromMessage for Error {
    fn from_message(message: String) -> Self {
        Self::Message(message)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

hermes_error::impl_context!();
