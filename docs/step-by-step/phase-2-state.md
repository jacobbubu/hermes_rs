# Phase 2: Session 存储 (hermes-state)

**状态**: 待实施
**Issue**: #1
**依赖**: Phase 1 (hermes-types, hermes-error)

## 目标

实现 SQLite session 数据库，精确匹配 Python `hermes_state.py` 的 schema 和行为。使用 `sqlx`（async，与 Moltis 一致）。

## 对照源码

**Python 实现**: `/Users/rongshen/github/hermes-agent/hermes_state.py` (~1,304 行)
**Moltis 参考**: `/Users/rongshen/github/moltis/crates/sessions/`

## 计划步骤

### Step 2.1: Schema 初始化 (sqlx migration)

**做什么**: 创建 `hermes-state` crate，用 sqlx migration 文件定义 SQLite schema。

**SQL schema** (逐字复制自 Python):

```sql
-- sessions 表 (26 列)
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    user_id TEXT,
    model TEXT,
    model_config TEXT,
    system_prompt TEXT,
    parent_session_id TEXT,
    started_at REAL NOT NULL,
    ended_at REAL,
    end_reason TEXT,
    message_count INTEGER DEFAULT 0,
    tool_call_count INTEGER DEFAULT 0,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    cache_read_tokens INTEGER DEFAULT 0,
    cache_write_tokens INTEGER DEFAULT 0,
    reasoning_tokens INTEGER DEFAULT 0,
    billing_provider TEXT,
    billing_base_url TEXT,
    billing_mode TEXT,
    estimated_cost_usd REAL,
    actual_cost_usd REAL,
    cost_status TEXT,
    cost_source TEXT,
    pricing_version TEXT,
    title TEXT,
    FOREIGN KEY (parent_session_id) REFERENCES sessions(id)
);

-- messages 表 (13 列)
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id),
    role TEXT NOT NULL,
    content TEXT,
    tool_call_id TEXT,
    tool_calls TEXT,       -- JSON array of ToolCall
    tool_name TEXT,
    timestamp REAL NOT NULL,
    token_count INTEGER,
    finish_reason TEXT,
    reasoning TEXT,
    reasoning_details TEXT, -- JSON
    codex_reasoning_items TEXT -- JSON
);

-- FTS5 全文搜索
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    content, content=messages, content_rowid=id
);

-- 触发器保持 FTS 同步
CREATE TRIGGER messages_fts_insert AFTER INSERT ON messages ...
CREATE TRIGGER messages_fts_delete AFTER DELETE ON messages ...
CREATE TRIGGER messages_fts_update AFTER UPDATE ON messages ...

-- 索引
CREATE INDEX idx_sessions_source ON sessions(source);
CREATE INDEX idx_sessions_parent ON sessions(parent_session_id);
CREATE INDEX idx_sessions_started ON sessions(started_at DESC);
CREATE INDEX idx_messages_session ON messages(session_id, timestamp);
```

**关键配置**:
- `PRAGMA journal_mode=WAL` — 并发读写
- `PRAGMA foreign_keys=ON` — 外键约束

**Rust 代码结构**:
```rust
pub struct SessionDb {
    pool: SqlitePool,
}

impl SessionDb {
    pub async fn new(db_path: &str) -> Result<Self>;
    // 内部调用 sqlx::migrate!() 执行 migration
}
```

**验证**: 创建临时 DB，检查所有表和索引存在。

### Step 2.2: 写入竞争处理

**做什么**: sqlx `SqlitePool` 自身管理连接池和并发，但需要：
- WAL 模式设置（连接时执行 PRAGMA）
- 适当的事务隔离（`BEGIN IMMEDIATE` 用于写操作）
- 定期 WAL checkpoint

**Python 原实现**: 手动 jitter retry（15 次，20-150ms），因为 Python sqlite3 是单连接。sqlx 的连接池让这个问题简单很多。

**Rust 代码**:
```rust
// 连接时设置 PRAGMA
let pool = SqlitePoolOptions::new()
    .after_connect(|conn, _meta| Box::pin(async move {
        conn.execute("PRAGMA journal_mode=WAL").await?;
        conn.execute("PRAGMA foreign_keys=ON").await?;
        Ok(())
    }))
    .connect(url).await?;
```

### Step 2.3: create_session / end_session

**做什么**: Session 生命周期方法。

```rust
impl SessionDb {
    pub async fn create_session(&self, session: &Session) -> Result<()>;
    pub async fn end_session(&self, id: &str, end_reason: &str) -> Result<()>;
    pub async fn get_session(&self, id: &str) -> Result<Option<Session>>;
}
```

**测试**:
1. 创建 session → 验证存在
2. 结束 session → 验证 `ended_at` 和 `end_reason` 已设置
3. 创建子 session（`parent_session_id`）→ 验证外键关系

### Step 2.4: append_message / get_messages

**做什么**: 消息追加和检索。

```rust
impl SessionDb {
    pub async fn append_message(&self, msg: &Message) -> Result<i64>;
    pub async fn get_messages(&self, session_id: &str) -> Result<Vec<Message>>;
}
```

**关键细节**:
- `append_message` 在 INSERT 后更新 session 的 `message_count` 计数器
- 如果消息包含 `tool_calls`，同时更新 `tool_call_count`
- `tool_calls` 字段存为 JSON TEXT（`serde_json::to_string`）
- `get_messages` 按 `timestamp` 排序返回
- FTS5 触发器自动同步（INSERT trigger）

**测试**:
1. 追加各种 role 的消息
2. 检索并验证顺序、内容、tool_calls JSON
3. 验证计数器自动递增

### Step 2.5: search_messages (FTS5)

**做什么**: 全文搜索。

```rust
impl SessionDb {
    pub async fn search_messages(
        &self,
        query: &str,
        session_id: Option<&str>,
    ) -> Result<Vec<SearchResult>>;
}

pub struct SearchResult {
    pub message: Message,
    pub snippet: String,
    pub rank: f64,
}
```

**关键细节**:
- FTS5 查询清洗：移除特殊字符，防止 SQL 注入
- 可选按 session_id 过滤
- 返回 snippet（高亮匹配片段）和 rank（相关度）

**测试**:
1. 插入多条消息 → 搜索关键词 → 验证匹配
2. 按 session 过滤 → 只返回该 session 的结果
3. 搜索不存在的词 → 空结果

## Rust 新概念

| 概念 | 说明 | 在哪里用到 |
|------|------|-----------|
| `async/await` | 异步编程，函数返回 Future | 所有 DB 操作 |
| `sqlx` | 编译时检查的异步 SQL 库 | SessionDb 的所有方法 |
| `SqlitePool` | 连接池，管理多个并发连接 | `SessionDb::new()` |
| `sqlx::migrate!()` | 编译时嵌入 SQL migration 文件 | schema 初始化 |
| `#[tokio::test]` | 异步测试宏 | 所有测试函数 |
| `tempfile` | 创建临时文件/目录，测试后自动清理 | 测试中的临时 DB |
| `Mutex` | 互斥锁（如果需要单写入者语义） | 可能用于 checkpoint |

## 目录结构（预期）

```
crates/hermes-state/
├── Cargo.toml
├── migrations/
│   └── 001_init.sql          # 完整 schema DDL
└── src/
    ├── lib.rs                # SessionDb 公开 API
    ├── error.rs              # crate 级错误类型
    ├── session_ops.rs        # create/end/get session
    ├── message_ops.rs        # append/get messages
    └── search.rs             # FTS5 搜索
```

## 质量门

- [ ] `cargo test -p hermes-state` 全部通过
- [ ] `cargo clippy` 无警告
- [ ] `cargo fmt --check` 通过
- [ ] **交叉验证**: Python 创建的 DB → Rust 能读取；Rust 创建的 DB → Python 能读取
- [ ] 多任务并发写入测试通过

## 与 Python 对照

| Rust 方法 | Python 方法 | 行为差异 |
|-----------|------------|----------|
| `SessionDb::new()` | `SessionDB.__init__()` | sqlx 连接池 vs 单连接 + threading.Lock |
| `create_session()` | `create_session()` | 相同 SQL |
| `end_session()` | `end_session()` | 相同 SQL |
| `append_message()` | `append_message()` | 相同 SQL + 计数器更新 |
| `get_messages()` | `get_messages()` | 相同 SQL，按 timestamp 排序 |
| `search_messages()` | `search_messages()` | 相同 FTS5 查询，同样的清洗逻辑 |
| (连接池管理) | `_execute_write()` jitter retry | sqlx pool 替代手动重试 |
