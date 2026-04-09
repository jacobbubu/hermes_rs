//! SQLite implementation of [`SessionStore`].

use async_trait::async_trait;
use sqlx::{Row, SqlitePool, sqlite::SqliteConnectOptions};
use std::str::FromStr;

use hermes_types::{Message, Role, Session, ToolCall};

use crate::error::{Context, Result};
use crate::store::{SearchResult, SessionStore};

/// SQLite-backed session store.
///
/// Uses an `sqlx::SqlitePool` for connection pooling and WAL mode for
/// concurrent read/write access. This is the first (and default) implementation
/// of [`SessionStore`].
pub struct SqliteSessionStore {
    pool: SqlitePool,
}

impl SqliteSessionStore {
    /// Open (or create) a SQLite database at the given path and run migrations.
    pub async fn new(db_path: &str) -> Result<Self> {
        let opts = SqliteConnectOptions::from_str(db_path)
            .context("parsing database path")?
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .foreign_keys(true);

        let pool = SqlitePool::connect_with(opts)
            .await
            .context("connecting to database")?;

        // Run migrations (embedded at compile time).
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .context("running migrations")?;

        Ok(Self { pool })
    }

    /// Access the underlying pool (for testing or advanced use).
    #[cfg(test)]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[async_trait]
impl SessionStore for SqliteSessionStore {
    async fn create_session(&self, session: &Session) -> Result<()> {
        sqlx::query(
            "INSERT INTO sessions (id, source, user_id, model, model_config, system_prompt,
             parent_session_id, started_at, ended_at, end_reason,
             message_count, tool_call_count, input_tokens, output_tokens,
             cache_read_tokens, cache_write_tokens, reasoning_tokens,
             billing_provider, billing_base_url, billing_mode,
             estimated_cost_usd, actual_cost_usd, cost_status, cost_source,
             pricing_version, title)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26)",
        )
        .bind(&session.id)
        .bind(&session.source)
        .bind(&session.user_id)
        .bind(&session.model)
        .bind(&session.model_config)
        .bind(&session.system_prompt)
        .bind(&session.parent_session_id)
        .bind(session.started_at)
        .bind(session.ended_at)
        .bind(&session.end_reason)
        .bind(session.message_count)
        .bind(session.tool_call_count)
        .bind(session.input_tokens)
        .bind(session.output_tokens)
        .bind(session.cache_read_tokens)
        .bind(session.cache_write_tokens)
        .bind(session.reasoning_tokens)
        .bind(&session.billing_provider)
        .bind(&session.billing_base_url)
        .bind(&session.billing_mode)
        .bind(session.estimated_cost_usd)
        .bind(session.actual_cost_usd)
        .bind(&session.cost_status)
        .bind(&session.cost_source)
        .bind(&session.pricing_version)
        .bind(&session.title)
        .execute(&self.pool)
        .await
        .context("inserting session")?;
        Ok(())
    }

    async fn get_session(&self, id: &str) -> Result<Option<Session>> {
        let row = sqlx::query("SELECT * FROM sessions WHERE id = ?1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .context("fetching session")?;

        match row {
            Some(r) => Ok(Some(session_from_row(&r))),
            None => Ok(None),
        }
    }

    async fn end_session(&self, id: &str, end_reason: &str) -> Result<()> {
        let now = now_secs();
        sqlx::query("UPDATE sessions SET ended_at = ?1, end_reason = ?2 WHERE id = ?3")
            .bind(now)
            .bind(end_reason)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("ending session")?;
        Ok(())
    }

    async fn list_sessions(&self, source: Option<&str>, limit: i64) -> Result<Vec<Session>> {
        let rows =
            match source {
                Some(src) => sqlx::query(
                    "SELECT * FROM sessions WHERE source = ?1 ORDER BY started_at DESC LIMIT ?2",
                )
                .bind(src)
                .bind(limit)
                .fetch_all(&self.pool)
                .await,
                None => {
                    sqlx::query("SELECT * FROM sessions ORDER BY started_at DESC LIMIT ?1")
                        .bind(limit)
                        .fetch_all(&self.pool)
                        .await
                },
            }
            .context("listing sessions")?;

        Ok(rows.iter().map(session_from_row).collect())
    }

    async fn append_message(&self, msg: &Message) -> Result<i64> {
        let tool_calls_json = msg
            .tool_calls
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .context("serializing tool_calls")?;

        let reasoning_details_json = msg
            .reasoning_details
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .context("serializing reasoning_details")?;

        let codex_json = msg
            .codex_reasoning_items
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .context("serializing codex_reasoning_items")?;

        let role_str = serde_json::to_string(&msg.role).context("serializing role")?;
        // Remove quotes: "\"user\"" -> "user"
        let role_str = role_str.trim_matches('"');

        let row_id: i64 = sqlx::query_scalar(
            "INSERT INTO messages (session_id, role, content, tool_call_id, tool_calls,
             tool_name, timestamp, token_count, finish_reason, reasoning,
             reasoning_details, codex_reasoning_items)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             RETURNING id",
        )
        .bind(&msg.session_id)
        .bind(role_str)
        .bind(&msg.content)
        .bind(&msg.tool_call_id)
        .bind(&tool_calls_json)
        .bind(&msg.tool_name)
        .bind(msg.timestamp)
        .bind(msg.token_count)
        .bind(&msg.finish_reason)
        .bind(&msg.reasoning)
        .bind(&reasoning_details_json)
        .bind(&codex_json)
        .fetch_one(&self.pool)
        .await
        .context("inserting message")?;

        // Update session counters.
        let tool_call_inc: i64 = match &msg.tool_calls {
            Some(tc) if !tc.is_empty() => tc.len() as i64,
            _ => 0,
        };

        sqlx::query(
            "UPDATE sessions SET message_count = message_count + 1,
             tool_call_count = tool_call_count + ?1 WHERE id = ?2",
        )
        .bind(tool_call_inc)
        .bind(&msg.session_id)
        .execute(&self.pool)
        .await
        .context("updating session counters")?;

        Ok(row_id)
    }

    async fn get_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let rows =
            sqlx::query("SELECT * FROM messages WHERE session_id = ?1 ORDER BY timestamp ASC")
                .bind(session_id)
                .fetch_all(&self.pool)
                .await
                .context("fetching messages")?;

        rows.iter().map(message_from_row).collect()
    }

    async fn search_messages(
        &self,
        query: &str,
        session_id: Option<&str>,
    ) -> Result<Vec<SearchResult>> {
        let sanitized = sanitize_fts5_query(query);
        if sanitized.is_empty() {
            return Ok(vec![]);
        }

        let rows = match session_id {
            Some(sid) => {
                sqlx::query(
                    "SELECT m.*, snippet(messages_fts, 0, '<b>', '</b>', '...', 32) AS snippet,
                     rank
                     FROM messages_fts
                     JOIN messages m ON m.id = messages_fts.rowid
                     WHERE messages_fts MATCH ?1 AND m.session_id = ?2
                     ORDER BY rank LIMIT 50",
                )
                .bind(&sanitized)
                .bind(sid)
                .fetch_all(&self.pool)
                .await
            },
            None => {
                sqlx::query(
                    "SELECT m.*, snippet(messages_fts, 0, '<b>', '</b>', '...', 32) AS snippet,
                     rank
                     FROM messages_fts
                     JOIN messages m ON m.id = messages_fts.rowid
                     WHERE messages_fts MATCH ?1
                     ORDER BY rank LIMIT 50",
                )
                .bind(&sanitized)
                .fetch_all(&self.pool)
                .await
            },
        }
        .context("searching messages")?;

        rows.iter()
            .map(|r| {
                let msg = message_from_row(r)?;
                let snippet: String = r.get("snippet");
                let rank: f64 = r.get("rank");
                Ok(SearchResult {
                    message: msg,
                    snippet,
                    rank,
                })
            })
            .collect()
    }

    async fn update_session_tokens(
        &self,
        id: &str,
        input_tokens: i64,
        output_tokens: i64,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE sessions SET input_tokens = input_tokens + ?1,
             output_tokens = output_tokens + ?2 WHERE id = ?3",
        )
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(id)
        .execute(&self.pool)
        .await
        .context("updating session tokens")?;
        Ok(())
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn session_from_row(r: &sqlx::sqlite::SqliteRow) -> Session {
    Session {
        id: r.get("id"),
        source: r.get("source"),
        user_id: r.get("user_id"),
        model: r.get("model"),
        model_config: r.get("model_config"),
        system_prompt: r.get("system_prompt"),
        parent_session_id: r.get("parent_session_id"),
        started_at: r.get("started_at"),
        ended_at: r.get("ended_at"),
        end_reason: r.get("end_reason"),
        message_count: r.get("message_count"),
        tool_call_count: r.get("tool_call_count"),
        input_tokens: r.get("input_tokens"),
        output_tokens: r.get("output_tokens"),
        cache_read_tokens: r.get("cache_read_tokens"),
        cache_write_tokens: r.get("cache_write_tokens"),
        reasoning_tokens: r.get("reasoning_tokens"),
        billing_provider: r.get("billing_provider"),
        billing_base_url: r.get("billing_base_url"),
        billing_mode: r.get("billing_mode"),
        estimated_cost_usd: r.get("estimated_cost_usd"),
        actual_cost_usd: r.get("actual_cost_usd"),
        cost_status: r.get("cost_status"),
        cost_source: r.get("cost_source"),
        pricing_version: r.get("pricing_version"),
        title: r.get("title"),
    }
}

fn message_from_row(r: &sqlx::sqlite::SqliteRow) -> Result<Message> {
    let role_str: String = r.get("role");
    let role: Role =
        serde_json::from_str(&format!("\"{role_str}\"")).context("parsing role from DB")?;

    let tool_calls_json: Option<String> = r.get("tool_calls");
    let tool_calls: Option<Vec<ToolCall>> = tool_calls_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .context("parsing tool_calls from DB")?;

    let reasoning_details_json: Option<String> = r.get("reasoning_details");
    let reasoning_details: Option<serde_json::Value> = reasoning_details_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .context("parsing reasoning_details from DB")?;

    let codex_json: Option<String> = r.get("codex_reasoning_items");
    let codex_reasoning_items: Option<serde_json::Value> = codex_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .context("parsing codex_reasoning_items from DB")?;

    Ok(Message {
        id: r.get("id"),
        session_id: r.get("session_id"),
        role,
        content: r.get("content"),
        tool_call_id: r.get("tool_call_id"),
        tool_calls,
        tool_name: r.get("tool_name"),
        timestamp: r.get("timestamp"),
        token_count: r.get("token_count"),
        finish_reason: r.get("finish_reason"),
        reasoning: r.get("reasoning"),
        reasoning_details,
        codex_reasoning_items,
    })
}

/// Sanitize a user query for FTS5 to prevent syntax errors.
///
/// Removes FTS5 special characters and wraps each word in quotes.
fn sanitize_fts5_query(query: &str) -> String {
    query
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .map(|w| format!("\"{w}\""))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hermes_types::ToolFunction;

    async fn temp_store() -> (SqliteSessionStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap_or_else(|e| panic!("tempdir: {e}"));
        let db_path = format!("sqlite:{}", dir.path().join("test.db").display());
        let store = SqliteSessionStore::new(&db_path)
            .await
            .unwrap_or_else(|e| panic!("new store: {e}"));
        (store, dir)
    }

    fn sample_session(id: &str) -> Session {
        Session {
            id: id.into(),
            source: "cli".into(),
            user_id: None,
            model: Some("test-model".into()),
            model_config: None,
            system_prompt: None,
            parent_session_id: None,
            started_at: 1712000000.0,
            ended_at: None,
            end_reason: None,
            message_count: 0,
            tool_call_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            billing_provider: None,
            billing_base_url: None,
            billing_mode: None,
            estimated_cost_usd: None,
            actual_cost_usd: None,
            cost_status: None,
            cost_source: None,
            pricing_version: None,
            title: None,
        }
    }

    fn sample_message(session_id: &str, role: Role, content: &str, ts: f64) -> Message {
        Message {
            id: None,
            session_id: session_id.into(),
            role,
            content: Some(content.into()),
            tool_call_id: None,
            tool_calls: None,
            tool_name: None,
            timestamp: ts,
            token_count: None,
            finish_reason: None,
            reasoning: None,
            reasoning_details: None,
            codex_reasoning_items: None,
        }
    }

    // ── Session tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn create_and_get_session() {
        let (store, _dir) = temp_store().await;
        let s = sample_session("s1");
        store
            .create_session(&s)
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        let got = store
            .get_session("s1")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(got.is_some());
        let got = got.unwrap_or_else(|| panic!("expected session"));
        assert_eq!(got.id, "s1");
        assert_eq!(got.source, "cli");
        assert_eq!(got.model.as_deref(), Some("test-model"));
    }

    #[tokio::test]
    async fn get_missing_session_returns_none() {
        let (store, _dir) = temp_store().await;
        let got = store
            .get_session("nonexistent")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn end_session_sets_fields() {
        let (store, _dir) = temp_store().await;
        store
            .create_session(&sample_session("s1"))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        store
            .end_session("s1", "completed")
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        let got = store
            .get_session("s1")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let got = got.unwrap_or_else(|| panic!("expected session"));
        assert!(got.ended_at.is_some());
        assert_eq!(got.end_reason.as_deref(), Some("completed"));
    }

    #[tokio::test]
    async fn list_sessions_filters_by_source() {
        let (store, _dir) = temp_store().await;
        let mut s1 = sample_session("s1");
        s1.source = "cli".into();
        let mut s2 = sample_session("s2");
        s2.source = "telegram".into();

        store
            .create_session(&s1)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        store
            .create_session(&s2)
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        let cli = store
            .list_sessions(Some("cli"), 10)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(cli.len(), 1);
        assert_eq!(cli[0].id, "s1");

        let all = store
            .list_sessions(None, 10)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(all.len(), 2);
    }

    // ── Message tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn append_and_get_messages() {
        let (store, _dir) = temp_store().await;
        store
            .create_session(&sample_session("s1"))
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        let m1 = sample_message("s1", Role::User, "hello", 1.0);
        let m2 = sample_message("s1", Role::Assistant, "hi there", 2.0);

        store
            .append_message(&m1)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        store
            .append_message(&m2)
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        let msgs = store
            .get_messages("s1")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, Role::User);
        assert_eq!(msgs[0].content.as_deref(), Some("hello"));
        assert_eq!(msgs[1].role, Role::Assistant);
    }

    #[tokio::test]
    async fn append_message_increments_counters() {
        let (store, _dir) = temp_store().await;
        store
            .create_session(&sample_session("s1"))
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        store
            .append_message(&sample_message("s1", Role::User, "q", 1.0))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        store
            .append_message(&sample_message("s1", Role::Assistant, "a", 2.0))
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        let s = store
            .get_session("s1")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let s = s.unwrap_or_else(|| panic!("expected session"));
        assert_eq!(s.message_count, 2);
    }

    #[tokio::test]
    async fn append_message_with_tool_calls() {
        let (store, _dir) = temp_store().await;
        store
            .create_session(&sample_session("s1"))
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        let mut msg = sample_message("s1", Role::Assistant, "", 1.0);
        msg.tool_calls = Some(vec![ToolCall {
            id: "call_1".into(),
            call_type: "function".into(),
            function: ToolFunction {
                name: "terminal".into(),
                arguments: r#"{"cmd":"ls"}"#.into(),
            },
        }]);
        msg.finish_reason = Some("tool_calls".into());

        store
            .append_message(&msg)
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        let s = store
            .get_session("s1")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let s = s.unwrap_or_else(|| panic!("expected session"));
        assert_eq!(s.tool_call_count, 1);

        let msgs = store
            .get_messages("s1")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(msgs[0].tool_calls.as_ref().map(|tc| tc.len()), Some(1));
    }

    // ── Search tests ───────────────────────────────────────────────────

    #[tokio::test]
    async fn search_finds_matching_messages() {
        let (store, _dir) = temp_store().await;
        store
            .create_session(&sample_session("s1"))
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        store
            .append_message(&sample_message(
                "s1",
                Role::User,
                "tell me about rust programming",
                1.0,
            ))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        store
            .append_message(&sample_message(
                "s1",
                Role::Assistant,
                "Rust is a systems language",
                2.0,
            ))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        store
            .append_message(&sample_message("s1", Role::User, "what about python", 3.0))
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        let results = store
            .search_messages("rust", None)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn search_filters_by_session() {
        let (store, _dir) = temp_store().await;
        store
            .create_session(&sample_session("s1"))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        store
            .create_session(&sample_session("s2"))
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        store
            .append_message(&sample_message("s1", Role::User, "hello world", 1.0))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        store
            .append_message(&sample_message("s2", Role::User, "hello universe", 2.0))
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        let results = store
            .search_messages("hello", Some("s1"))
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].message.session_id, "s1");
    }

    #[tokio::test]
    async fn search_empty_query_returns_empty() {
        let (store, _dir) = temp_store().await;
        let results = store
            .search_messages("", None)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(results.is_empty());
    }

    // ── Token update test ──────────────────────────────────────────────

    #[tokio::test]
    async fn update_session_tokens_accumulates() {
        let (store, _dir) = temp_store().await;
        store
            .create_session(&sample_session("s1"))
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        store
            .update_session_tokens("s1", 100, 50)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        store
            .update_session_tokens("s1", 200, 80)
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        let s = store
            .get_session("s1")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let s = s.unwrap_or_else(|| panic!("expected session"));
        assert_eq!(s.input_tokens, 300);
        assert_eq!(s.output_tokens, 130);
    }
}
