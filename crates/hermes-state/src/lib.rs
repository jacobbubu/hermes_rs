//! Session storage abstraction and SQLite implementation for hermes_rs.
//!
//! This crate provides:
//! - [`SessionStore`] trait — the storage abstraction upper layers depend on.
//! - [`SqliteSessionStore`] — the SQLite implementation (first and default backend).
//! - [`SearchResult`] — full-text search result type.
//!
//! # Architecture
//!
//! Python `hermes_state.py` directly couples to `sqlite3`. This crate introduces
//! a trait abstraction so the storage backend can be swapped (e.g., PostgreSQL)
//! without changing upper-layer code.
//!
//! ```text
//! Agent Loop / CLI / Gateway
//!         │
//!         ▼
//!   Box<dyn SessionStore>      ← only knows the trait
//!         │
//!         ▼
//!   SqliteSessionStore         ← first implementation
//!         │
//!         ▼
//!      SQLite (via sqlx)
//! ```

pub mod error;
mod sqlite;
mod store;

pub use sqlite::SqliteSessionStore;
pub use store::{SearchResult, SessionStore};
