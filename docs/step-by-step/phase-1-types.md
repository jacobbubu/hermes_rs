# Phase 1: 核心数据类型

**状态**: 已完成 (2026-04-09)
**Issue**: #1
**Commit**: `4bf7700`

## 目标

定义所有消息、工具调用、session 元数据类型和错误处理基础设施。这些类型是整个系统的通用语言，必须与 Python `hermes_state.py` 的 SQLite schema 精确匹配。

## 做了什么

### Step 1.1-1.2: Message、Role、ToolCall、ToolFunction

**文件**: `crates/hermes-types/src/message.rs`

```rust
// Role enum — 序列化为小写字符串
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System, User, Assistant, Tool,
}

// ToolCall — 匹配 OpenAI function calling 格式
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]      // JSON 中是 "type"，Rust 中用 call_type
    pub call_type: String,
    pub function: ToolFunction,
}

// Message — 精确匹配 Python messages 表的每一列
pub struct Message {
    pub id: Option<i64>,           // DB 自增 id
    pub session_id: String,
    pub role: Role,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub timestamp: f64,            // Unix 秒，f64
    // ... 其余字段
}
```

关键设计决策：
- 用 `#[serde(skip_serializing_if = "Option::is_none")]` 让 None 字段在 JSON 中不出现
- `timestamp` 用 `f64` 而非 `i64`，匹配 Python 的 `REAL` 列类型
- `tool_calls` 存为 `Vec<ToolCall>` 而非 JSON string，序列化时自动处理

### Step 1.3: Session 元数据

**文件**: `crates/hermes-types/src/session.rs`

精确匹配 Python `sessions` 表的 26 个字段。计数器字段用 `#[serde(default)]` 确保缺失时默认为 0。

### Step 1.4: ToolSchema、ToolEntry、ToolSource

**文件**: `crates/hermes-types/src/tool.rs`

```rust
// 工具来源 — Builtin / MCP / WASM
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolSource {
    Builtin,
    Mcp { server: String },
    Wasm,
}
```

### Step 1.5: hermes-error crate

**文件**: `crates/hermes-error/src/lib.rs`

参考 Moltis 的 `moltis-common` 错误处理模式：

```rust
// 共享错误类型
#[derive(Error, Debug)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("internal error")]
    Other { source: Box<dyn std::error::Error + Send + Sync> },
}

// FromMessage trait — 让任何 crate 的错误类型都能从 String 构造
pub trait FromMessage: Sized {
    fn from_message(message: String) -> Self;
}

// impl_context! 宏 — 给 Result 和 Option 加 .context() 方法
hermes_error::impl_context!();

// 使用示例：
let file = std::fs::read_to_string(path).context("loading config")?;
let value = some_option.context("expected a value")?;
```

## Rust 概念

| 概念 | 在哪里用到 | 说明 |
|------|-----------|------|
| `struct` | Message, Session, ToolCall | 数据结构体 |
| `enum` | Role, ToolSource | 枚举类型（Rust 的 enum 比 Python 的强大很多，可以带数据） |
| `derive` 宏 | `#[derive(Debug, Clone, Serialize)]` | 自动生成 trait 实现 |
| `serde` | 所有类型 | 序列化/反序列化框架，`#[serde(rename)]` 控制 JSON 字段名 |
| `Option<T>` | 所有可选字段 | Rust 没有 null，用 `Option<T>` 表示"可能没有值" |
| `Vec<T>` | `tool_calls: Vec<ToolCall>` | 动态数组（类似 Python list） |
| `#[cfg(test)]` | 测试模块 | 条件编译，测试代码只在 `cargo test` 时编译 |
| `thiserror` | hermes-error | 用 derive 宏自动生成 `std::error::Error` 实现 |
| 宏 (`macro_rules!`) | `impl_context!` | Rust 的卫生宏，生成 `.context()` 方法 |

## 测试

17 个测试，覆盖：
- Role 序列化为小写字符串
- ToolCall 的 `type` 字段重命名
- Message 的 serde 往返（assistant + tool_calls、tool result）
- None 字段在 JSON 中省略
- Session 的 serde 往返（含计数器、billing、默认值）
- ToolSchema 匹配 OpenAI 格式
- Error 类型的 display、context、类型转换

```bash
cargo test --workspace
# running 17 tests ... ok
```

## 目录结构

```
hermes_rs/
├── crates/
│   ├── hermes-error/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       └── lib.rs          # Error, FromMessage, impl_context!
│   └── hermes-types/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs          # 模块导出
│           ├── message.rs      # Message, Role, ToolCall, ToolFunction
│           ├── session.rs      # Session
│           └── tool.rs         # ToolSchema, ToolEntry, ToolSource
└── ...
```

## 与 Python 对照

| Rust 类型 | Python 来源 | 验证方式 |
|-----------|------------|----------|
| `Role` | `messages.role` 列 | 序列化为 "system"/"user"/"assistant"/"tool" |
| `Message` | `messages` 表 | 字段名精确匹配列名 |
| `ToolCall` | `messages.tool_calls` JSON | OpenAI function calling 格式 |
| `Session` | `sessions` 表 | 26 个字段精确匹配 |
| `ToolSchema` | `tools/registry.py` | OpenAI `{"type":"function","function":{...}}` |
