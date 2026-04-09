//! Storage abstraction — upper layers depend on this trait, not on SQLite directly.

use async_trait::async_trait;
use hermes_types::{Message, Session};

use crate::error::Result;

/// A search result from full-text search.
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// The matched message.
    pub message: Message,
    /// FTS5 snippet with highlighted matches.
    pub snippet: String,
    /// Relevance rank (lower is better).
    pub rank: f64,
}

/// Storage abstraction for sessions and messages.
///
/// Upper-layer code (agent loop, CLI, gateway) depends on this trait only.
/// [`super::SqliteSessionStore`] is the first (and currently only) implementation.
///
/// # Design notes
///
/// - All methods are `async` because database I/O is inherently async.
/// - `&self` (not `&mut self`) allows sharing one store across concurrent tasks.
/// - `Send + Sync` bounds let the store be held in an `Arc` and shared across threads.
/// - Error type is crate-local; callers convert via `?` or `.context()`.
#[async_trait]
pub trait SessionStore: Send + Sync {
    // ── Session lifecycle ──────────────────────────────────────────────

    /// Insert a new session record.
    async fn create_session(&self, session: &Session) -> Result<()>;

    /// Retrieve a session by id. Returns `None` if not found.
    async fn get_session(&self, id: &str) -> Result<Option<Session>>;

    /// Mark a session as ended with the given reason.
    async fn end_session(&self, id: &str, end_reason: &str) -> Result<()>;

    /// List sessions, optionally filtered by source, newest first.
    async fn list_sessions(&self, source: Option<&str>, limit: i64) -> Result<Vec<Session>>;

    // ── Messages ───────────────────────────────────────────────────────

    /// Append a message and return its auto-generated row id.
    ///
    /// Also increments `message_count` (and `tool_call_count` if applicable)
    /// on the parent session.
    async fn append_message(&self, msg: &Message) -> Result<i64>;

    /// Retrieve all messages for a session, ordered by timestamp.
    async fn get_messages(&self, session_id: &str) -> Result<Vec<Message>>;

    // ── Search ─────────────────────────────────────────────────────────

    /// Full-text search across message content.
    async fn search_messages(
        &self,
        query: &str,
        session_id: Option<&str>,
    ) -> Result<Vec<SearchResult>>;

    // ── Metadata updates ───────────────────────────────────────────────

    /// Update token counters on a session.
    async fn update_session_tokens(
        &self,
        id: &str,
        input_tokens: i64,
        output_tokens: i64,
    ) -> Result<()>;
}
