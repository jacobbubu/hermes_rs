//! Error types and context trait for hermes_rs.
//!
//! Provides a shared [`Error`] enum, a [`Result`] alias, and the
//! [`FromMessage`] trait + [`impl_context!`] macro for ergonomic error
//! propagation across crates.
//!
//! Pattern adapted from the Moltis project (`moltis-common`).

use thiserror::Error;

/// Shared error type for hermes_rs crates.
#[derive(Error, Debug)]
pub enum Error {
    /// A plain text error message.
    #[error("{0}")]
    Message(String),

    /// An I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// A JSON serialization/deserialization error.
    #[error(transparent)]
    Json(#[from] serde_json::Error),

    /// An opaque internal error wrapping any `std::error::Error`.
    #[error("internal error")]
    Other {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}

impl Error {
    /// Create an error from a plain message.
    #[must_use]
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }

    /// Wrap an arbitrary error as [`Error::Other`].
    #[must_use]
    pub fn other(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Other {
            source: Box::new(source),
        }
    }
}

impl FromMessage for Error {
    fn from_message(message: String) -> Self {
        Self::Message(message)
    }
}

/// Convenience alias.
pub type Result<T> = std::result::Result<T, Error>;

// ── Shared context trait ───────────────────────────────────────────────────

/// Trait for error types that can be constructed from a plain message string.
///
/// Implement this for your crate's error type, then invoke [`impl_context!`]
/// in your error module to get `.context()` and `.with_context()` on `Result`
/// and `Option`.
pub trait FromMessage: Sized {
    fn from_message(message: String) -> Self;
}

/// Generate a crate-local `Context` trait with `.context()` and
/// `.with_context()` methods on `Result` and `Option`.
///
/// Invoke inside a module that defines `Error: FromMessage` and
/// `type Result<T> = std::result::Result<T, Error>`.
///
/// ```ignore
/// // in crates/foo/src/error.rs
/// hermes_error::impl_context!();
/// ```
#[macro_export]
macro_rules! impl_context {
    () => {
        pub trait Context<T> {
            fn context(self, context: impl Into<String>) -> Result<T>;
            fn with_context<C, F>(self, f: F) -> Result<T>
            where
                C: Into<String>,
                F: FnOnce() -> C;
        }

        impl<T, E: std::fmt::Display> Context<T> for std::result::Result<T, E> {
            fn context(self, context: impl Into<String>) -> Result<T> {
                let ctx = context.into();
                self.map_err(|source| {
                    <Error as $crate::FromMessage>::from_message(format!("{ctx}: {source}"))
                })
            }

            fn with_context<C, F>(self, f: F) -> Result<T>
            where
                C: Into<String>,
                F: FnOnce() -> C,
            {
                self.map_err(|source| {
                    let ctx = f().into();
                    <Error as $crate::FromMessage>::from_message(format!("{ctx}: {source}"))
                })
            }
        }

        impl<T> Context<T> for Option<T> {
            fn context(self, context: impl Into<String>) -> Result<T> {
                self.ok_or_else(|| <Error as $crate::FromMessage>::from_message(context.into()))
            }

            fn with_context<C, F>(self, f: F) -> Result<T>
            where
                C: Into<String>,
                F: FnOnce() -> C,
            {
                self.ok_or_else(|| <Error as $crate::FromMessage>::from_message(f().into()))
            }
        }
    };
}

// Use our own macro to get Context on our own Result/Option.
impl_context!();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_message_displays() {
        let e = Error::message("something went wrong");
        assert_eq!(e.to_string(), "something went wrong");
    }

    #[test]
    fn result_context_adds_prefix() {
        let r: std::result::Result<(), std::io::Error> =
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "missing"));
        let mapped: Result<()> = r.context("loading config");
        let err_msg = mapped.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(err_msg.contains("loading config"));
        assert!(err_msg.contains("missing"));
    }

    #[test]
    fn option_context_converts_none() {
        let opt: Option<i32> = None;
        let r: Result<i32> = opt.context("expected a value");
        let err_msg = r.err().map(|e| e.to_string()).unwrap_or_default();
        assert_eq!(err_msg, "expected a value");
    }

    #[test]
    fn option_context_passes_some() {
        let opt: Option<i32> = Some(42);
        let r: Result<i32> = opt.context("should not fail");
        assert_eq!(r.ok(), Some(42));
    }

    #[test]
    fn io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let e: Error = io_err.into();
        assert!(e.to_string().contains("denied"));
    }
}
