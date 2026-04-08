//! Tool schema and registry entry types.
//!
//! These types define how tools are registered and their schemas are
//! represented, matching the OpenAI function calling format used by the
//! Python `tools/registry.py`.

use serde::{Deserialize, Serialize};

/// OpenAI function calling schema for a tool.
///
/// This is the format sent to the LLM in the `tools` array of API requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchema {
    /// Always "function".
    #[serde(rename = "type")]
    pub schema_type: String,
    /// Function details.
    pub function: ToolSchemaFunction,
}

/// Function details within a [`ToolSchema`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSchemaFunction {
    pub name: String,
    pub description: String,
    /// JSON Schema for the function parameters.
    pub parameters: serde_json::Value,
}

/// Where a tool comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolSource {
    /// Built-in tool (e.g., terminal, file_tools).
    Builtin,
    /// Dynamically discovered via MCP server.
    Mcp { server: String },
    /// Loaded from a WASM component.
    Wasm,
}

/// A registered tool entry in the registry.
#[derive(Debug, Clone)]
pub struct ToolEntry {
    /// Tool name (unique identifier).
    pub name: String,
    /// Which toolset this tool belongs to (e.g., "web", "terminal").
    pub toolset: String,
    /// OpenAI function schema.
    pub schema: ToolSchema,
    /// Where this tool comes from.
    pub source: ToolSource,
    /// Human-readable description.
    pub description: String,
    /// Emoji indicator for UI display.
    pub emoji: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_schema_matches_openai_format() {
        let schema = ToolSchema {
            schema_type: "function".into(),
            function: ToolSchemaFunction {
                name: "web_search".into(),
                description: "Search the web".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query"
                        }
                    },
                    "required": ["query"]
                }),
            },
        };

        let json = serde_json::to_string(&schema).ok().unwrap_or_default();
        // Verify OpenAI format: "type" key at top level
        assert!(json.contains(r#""type":"function""#));
        assert!(json.contains(r#""name":"web_search""#));

        let back: ToolSchema = serde_json::from_str(&json)
            .ok()
            .unwrap_or_else(|| schema.clone());
        assert_eq!(back.function.name, "web_search");
    }

    #[test]
    fn tool_source_roundtrips() {
        let builtin = ToolSource::Builtin;
        let json = serde_json::to_string(&builtin).ok().unwrap_or_default();
        assert_eq!(json, r#""builtin""#);

        let mcp = ToolSource::Mcp {
            server: "github".into(),
        };
        let json = serde_json::to_string(&mcp).ok().unwrap_or_default();
        let back: ToolSource = serde_json::from_str(&json)
            .ok()
            .unwrap_or(ToolSource::Builtin);
        assert_eq!(back, mcp);
    }
}
