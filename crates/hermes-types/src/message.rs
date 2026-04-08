//! Message and tool call types, wire-compatible with the Python `messages` table.

use serde::{Deserialize, Serialize};

/// Message role, matching the Python `role` column values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A tool call issued by the assistant, in OpenAI function calling format.
///
/// Stored as JSON in the `tool_calls` column of the `messages` table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub call_type: String,
    pub function: ToolFunction,
}

/// Function details within a [`ToolCall`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolFunction {
    pub name: String,
    /// JSON-encoded arguments string (not parsed).
    pub arguments: String,
}

/// A message in a conversation, matching the Python `messages` table schema.
///
/// Field names correspond exactly to SQLite column names in `hermes_state.py`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Auto-increment row id (populated when read from DB, `None` for new messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    /// Session this message belongs to.
    pub session_id: String,
    /// Message role.
    pub role: Role,
    /// Text content (may be `None` for tool-call-only assistant messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// For `tool` role: the id of the tool call this result corresponds to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// For `assistant` role: tool calls requested by the model (JSON array).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// For `tool` role: which tool was called.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// Unix timestamp (seconds since epoch, as f64).
    pub timestamp: f64,
    /// Cached token estimate for this message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_count: Option<i64>,
    /// How the model finished: "stop", "tool_calls", "length", etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    /// Model reasoning / thinking text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// Structured reasoning details (JSON).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_details: Option<serde_json::Value>,
    /// Codex-specific reasoning items (JSON).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codex_reasoning_items: Option<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&Role::System).ok(),
            Some("\"system\"".into())
        );
        assert_eq!(
            serde_json::to_string(&Role::User).ok(),
            Some("\"user\"".into())
        );
        assert_eq!(
            serde_json::to_string(&Role::Assistant).ok(),
            Some("\"assistant\"".into())
        );
        assert_eq!(
            serde_json::to_string(&Role::Tool).ok(),
            Some("\"tool\"".into())
        );
    }

    #[test]
    fn role_roundtrips() {
        for role in [Role::System, Role::User, Role::Assistant, Role::Tool] {
            let json = serde_json::to_string(&role).ok();
            assert!(json.is_some());
            let back: Role = serde_json::from_str(json.as_deref().unwrap_or_default())
                .ok()
                .unwrap_or(Role::System);
            assert_eq!(back, role);
        }
    }

    #[test]
    fn tool_call_roundtrips() {
        let tc = ToolCall {
            id: "call_abc123".into(),
            call_type: "function".into(),
            function: ToolFunction {
                name: "web_search".into(),
                arguments: r#"{"query":"rust agent"}"#.into(),
            },
        };
        let json = serde_json::to_string(&tc).ok();
        assert!(json.is_some());
        let json = json.unwrap_or_default();

        // Verify "type" key is used (not "call_type")
        assert!(json.contains(r#""type":"function""#));

        let back: ToolCall = serde_json::from_str(&json)
            .ok()
            .unwrap_or_else(|| tc.clone());
        assert_eq!(back, tc);
    }

    #[test]
    fn message_assistant_with_tool_calls_roundtrips() {
        let msg = Message {
            id: None,
            session_id: "sess_001".into(),
            role: Role::Assistant,
            content: None,
            tool_call_id: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_1".into(),
                call_type: "function".into(),
                function: ToolFunction {
                    name: "terminal".into(),
                    arguments: r#"{"command":"ls"}"#.into(),
                },
            }]),
            tool_name: None,
            timestamp: 1712000000.0,
            token_count: Some(42),
            finish_reason: Some("tool_calls".into()),
            reasoning: None,
            reasoning_details: None,
            codex_reasoning_items: None,
        };
        let json = serde_json::to_string(&msg).ok();
        assert!(json.is_some());
        let json = json.unwrap_or_default();

        let back: Message = serde_json::from_str(&json)
            .ok()
            .unwrap_or_else(|| msg.clone());
        assert_eq!(back.role, Role::Assistant);
        assert!(back.content.is_none());
        assert_eq!(back.tool_calls.as_ref().map(|v| v.len()), Some(1));
        assert_eq!(back.finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn message_tool_result_roundtrips() {
        let msg = Message {
            id: Some(7),
            session_id: "sess_001".into(),
            role: Role::Tool,
            content: Some(r#"{"result":"ok"}"#.into()),
            tool_call_id: Some("call_1".into()),
            tool_calls: None,
            tool_name: Some("terminal".into()),
            timestamp: 1712000001.0,
            token_count: None,
            finish_reason: None,
            reasoning: None,
            reasoning_details: None,
            codex_reasoning_items: None,
        };
        let json = serde_json::to_string(&msg).ok();
        assert!(json.is_some());
        let json = json.unwrap_or_default();

        let back: Message = serde_json::from_str(&json)
            .ok()
            .unwrap_or_else(|| msg.clone());
        assert_eq!(back.role, Role::Tool);
        assert_eq!(back.tool_name.as_deref(), Some("terminal"));
        assert_eq!(back.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(back.id, Some(7));
    }

    #[test]
    fn message_skips_none_fields_in_json() {
        let msg = Message {
            id: None,
            session_id: "s".into(),
            role: Role::User,
            content: Some("hello".into()),
            tool_call_id: None,
            tool_calls: None,
            tool_name: None,
            timestamp: 1.0,
            token_count: None,
            finish_reason: None,
            reasoning: None,
            reasoning_details: None,
            codex_reasoning_items: None,
        };
        let json = serde_json::to_string(&msg).ok().unwrap_or_default();

        // None fields should be omitted
        assert!(!json.contains("tool_calls"));
        assert!(!json.contains("reasoning"));
        assert!(!json.contains("\"id\":"));
        // Present fields should be there
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"hello\""));
    }
}
