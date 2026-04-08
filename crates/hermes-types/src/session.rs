//! Session metadata type, wire-compatible with the Python `sessions` table.

use serde::{Deserialize, Serialize};

/// A session record, matching the Python `sessions` table schema exactly.
///
/// All field names correspond to SQLite column names in `hermes_state.py`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Primary key (UUID string).
    pub id: String,
    /// Origin: "cli", "telegram", "discord", "cron", "gateway", etc.
    pub source: String,
    /// Gateway user identifier (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    /// Model name used for this session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// JSON string of provider configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_config: Option<String>,
    /// System prompt used (stored for prefix cache stability).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    /// Parent session id for compression-split chains.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    /// Unix timestamp when session started.
    pub started_at: f64,
    /// Unix timestamp when session ended (None if still active).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<f64>,
    /// Why the session ended: "completed", "user_interrupt", "error", etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_reason: Option<String>,

    // ── Counters ───────────────────────────────────────────────────────
    #[serde(default)]
    pub message_count: i64,
    #[serde(default)]
    pub tool_call_count: i64,
    #[serde(default)]
    pub input_tokens: i64,
    #[serde(default)]
    pub output_tokens: i64,
    #[serde(default)]
    pub cache_read_tokens: i64,
    #[serde(default)]
    pub cache_write_tokens: i64,
    #[serde(default)]
    pub reasoning_tokens: i64,

    // ── Billing ────────────────────────────────────────────────────────
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_cost_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_cost_usd: Option<f64>,
    /// "estimated", "finalized", or "pending".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing_version: Option<String>,

    // ── Display ────────────────────────────────────────────────────────
    /// Optional user-friendly session title (unique when set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_session() -> Session {
        Session {
            id: "sess_abc123".into(),
            source: "cli".into(),
            user_id: None,
            model: Some("anthropic/claude-opus-4".into()),
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

    #[test]
    fn session_roundtrips() {
        let s = sample_session();
        let json = serde_json::to_string(&s).ok().unwrap_or_default();
        let back: Session = serde_json::from_str(&json)
            .ok()
            .unwrap_or_else(|| s.clone());
        assert_eq!(back.id, "sess_abc123");
        assert_eq!(back.source, "cli");
        assert_eq!(back.model.as_deref(), Some("anthropic/claude-opus-4"));
        assert_eq!(back.message_count, 0);
    }

    #[test]
    fn session_skips_none_fields() {
        let s = sample_session();
        let json = serde_json::to_string(&s).ok().unwrap_or_default();
        assert!(!json.contains("user_id"));
        assert!(!json.contains("parent_session_id"));
        assert!(!json.contains("billing_provider"));
        assert!(!json.contains("title"));
    }

    #[test]
    fn session_with_billing_roundtrips() {
        let mut s = sample_session();
        s.ended_at = Some(1712003600.0);
        s.end_reason = Some("completed".into());
        s.message_count = 12;
        s.input_tokens = 5000;
        s.output_tokens = 1200;
        s.estimated_cost_usd = Some(0.042);
        s.cost_status = Some("estimated".into());
        s.title = Some("My research session".into());

        let json = serde_json::to_string(&s).ok().unwrap_or_default();
        let back: Session = serde_json::from_str(&json)
            .ok()
            .unwrap_or_else(|| s.clone());
        assert_eq!(back.end_reason.as_deref(), Some("completed"));
        assert_eq!(back.message_count, 12);
        assert_eq!(back.estimated_cost_usd, Some(0.042));
        assert_eq!(back.title.as_deref(), Some("My research session"));
    }

    #[test]
    fn session_defaults_counters_on_missing() {
        // Simulate JSON without counter fields (should default to 0)
        let json = r#"{"id":"s1","source":"cli","started_at":1.0}"#;
        let s: Session = serde_json::from_str(json)
            .ok()
            .unwrap_or_else(sample_session);
        assert_eq!(s.message_count, 0);
        assert_eq!(s.input_tokens, 0);
        assert_eq!(s.reasoning_tokens, 0);
    }
}
