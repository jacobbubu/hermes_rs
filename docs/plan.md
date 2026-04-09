# hermes_rs: 将 Hermes Agent Python 内核迁移到 Rust

## Context

Hermes Agent 的 Python 内核（~14,800 行，7 个模块）是一个功能完整但架构耦合的单体。`run_agent.py` 单文件 9,400 行，SessionDB 直接耦合 SQLite，没有存储抽象接口。用户希望将核心内核用 Rust 重写为 `hermes_rs`，通过 PyO3 桥接现有 Python CLI/Gateway，同时保持数据格式和协议的完全兼容。

工程质量参考 Moltis（46 crate workspace，deny unsafe/unwrap/expect，trait-based 架构）。用户不熟悉 Rust，需要一个极其缓慢、细致、每步可审查的实现路径。

**目标**：在 `/Users/rongshen/github/hermes_rs/` 创建 Rust workspace，最终作为 Hermes Agent 的 drop-in 内核替换。

## 兼容性约束

必须 wire-compatible 的格式：
- SQLite schema v6（sessions / messages / messages_fts 表，精确匹配列名和类型）
- 消息格式：OpenAI 风格 `{role, content, tool_calls}` 内部表示
- 三种 API 模式：`chat_completions` / `anthropic_messages` / `codex_responses`
- Memory 文件：`§` 分隔符，MEMORY.md 2200 字符 / USER.md 1375 字符限制
- Tool schema：OpenAI function calling JSON format
- Config：YAML + .env

## Crate 依赖图

```
hermes-error         (无内部依赖)
    ↑
hermes-types         (← hermes-error, serde, serde_json)
    ↑
hermes-model-meta    (← hermes-types, regex, reqwest)
    ↑
hermes-state         (← hermes-types, hermes-error, sqlx)
    ↑
hermes-memory        (← hermes-types, hermes-error, tracing)
    ↑
hermes-tools         (← hermes-types, hermes-error, serde_json)
    ↑
hermes-prompt        (← hermes-types, hermes-model-meta, regex)
    ↑
hermes-compressor    (← hermes-types, hermes-model-meta)
    ↑
hermes-providers     (← hermes-types, reqwest, async-trait, tokio)
    ↑
hermes-agent         (← 以上所有)
    ↑
hermes-pyo3          (← hermes-agent, pyo3, maturin)
```

---

## Phase 0: Workspace 脚手架

**目标**：空的、能编译的 Rust workspace，CI 基础设施就位。

### Step 0.1: 创建 workspace 根

创建 `/Users/rongshen/github/hermes_rs/Cargo.toml`：
- `resolver = "2"`, `edition = "2024"`, `rust-version = "1.85"`
- workspace lints: `unsafe_code = "deny"`, `unwrap_used = "deny"`, `expect_used = "deny"`
- workspace dependencies 预填充：serde, serde_json, thiserror, anyhow, tokio, sqlx (sqlite + runtime-tokio), uuid, regex, tracing, chrono

**参考**：`/Users/rongshen/github/moltis/Cargo.toml`（lines 124-137）

### Step 0.2: 创建第一个空 crate `hermes-types`

标准 crate 结构，`[lints] workspace = true`。

**参考**：`/Users/rongshen/github/moltis/crates/common/Cargo.toml`

### Step 0.3: 配置文件

- `rustfmt.toml`（从 Moltis 复制）
- `clippy.toml`（从 Moltis 复制）
- `rust-toolchain.toml`（pin stable 1.85）
- `justfile`：check / build / test / clippy / fmt / gate

**参考**：`/Users/rongshen/github/moltis/rustfmt.toml`, `/Users/rongshen/github/moltis/clippy.toml`

### Gate 0
- [x] `just gate` 通过（build + clippy + fmt + test） ✅ 2026-04-08

---

## Phase 1: 核心数据类型（hermes-types + hermes-error）

**目标**：定义所有消息、工具调用、session 元数据类型。

**Rust 概念**：struct, enum, derive 宏, serde, Option, Vec, 单元测试

### Step 1.1: Message 和 Role 类型

匹配 Python `messages` 表 schema：role, content, tool_call_id, tool_calls, tool_name, timestamp, token_count, finish_reason, reasoning, reasoning_details, codex_reasoning_items。

写 serde 往返测试。~150 行类型 + ~100 行测试。

**对照**：`/Users/rongshen/github/hermes-agent/hermes_state.py`（lines 71-85）
**参考**：`/Users/rongshen/github/moltis/crates/sessions/src/message.rs`

### Step 1.2: ToolCall 和 ToolFunction 类型

匹配 OpenAI function calling 格式。~50 行。

### Step 1.3: Session 元数据 struct

匹配 Python `sessions` 表所有 26 个字段。~100 行。

**对照**：`/Users/rongshen/github/hermes-agent/hermes_state.py`（lines 42-68）

### Step 1.4: ToolSchema / ToolEntry / ToolSource

匹配 registry.py 和 Moltis ToolSource enum。

**参考**：`/Users/rongshen/github/moltis/crates/agents/src/tool_registry.rs`（lines 23-38）

### Step 1.5: hermes-error crate

thiserror Error enum（Io / Json / Sqlite / Message / Config），Result 别名，FromMessage trait + impl_context! 宏。

**参考**：`/Users/rongshen/github/moltis/crates/common/src/error.rs`（完整文件）

### Gate 1
- [x] 每个类型有 serde 往返测试 ✅ 2026-04-09
- [x] 字段名精确匹配 Python 列名 ✅ 2026-04-09
- [x] `just gate` 通过 ✅ 2026-04-09 (17 tests)

---

## Phase 2: Session 存储（hermes-state）

**目标**：SQLite session DB，精确匹配 `hermes_state.py`。使用 sqlx（async，与 Moltis 一致）。

**Rust 概念**：sqlx (async SQLite), async/await 基础, Mutex, 闭包, ? 操作符, 错误传播, tempfile

### Step 2.1: Schema 初始化（sqlx migration）

从 Python 逐字复制 SQL DDL 到 sqlx migration 文件。WAL 模式 + foreign keys + FTS5。使用 `sqlx::sqlite::SqlitePool` 连接池。~200 行。

**对照**：`/Users/rongshen/github/hermes-agent/hermes_state.py`（lines 37-111）
**参考**：Moltis 使用 sqlx 的模式（`/Users/rongshen/github/moltis/crates/sessions/`）

### Step 2.2: 写入竞争处理

sqlx 连接池本身处理并发，但仍需 WAL checkpoint 和适当的事务隔离。

**对照**：`/Users/rongshen/github/hermes-agent/hermes_state.py`（lines 163-210）

### Step 2.3: create_session / end_session

Session 生命周期方法 + 测试。

### Step 2.4: append_message / get_messages

消息追加（INSERT + 计数器更新）和检索。验证 FTS5 触发器。

### Step 2.5: search_messages（FTS5）

FTS5 查询清洗、过滤、snippet 生成。

### Gate 2
- [x] 完整生命周期测试：create → append → search → end ✅ 2026-04-09
- [ ] **交叉验证**：Python 创建 DB → Rust 读取验证（反之亦然）（待后续验证）
- [ ] 多线程并发写入测试（待后续补充）
- [x] SessionStore trait 抽象层（Python 没有的改进） ✅ 2026-04-09
- [x] `just gate` 通过 ✅ 2026-04-09 (28 tests)

---

## Phase 3: 模型元数据和 Token 估算（hermes-model-meta）

**目标**：provider prefix 处理、默认 context length、token 估算。

**Rust 概念**：HashMap, regex, OnceLock, reqwest, Result/Option 链

### Step 3.1: Provider prefix 剥离和模型名规范化

纯字符串函数，表驱动测试。

### Step 3.2: 默认 context length 查找表

静态 HashMap + 模糊匹配 + probe tier。

### Step 3.3: Token 估算函数

estimate_tokens_rough（chars/4）、estimate_messages_tokens_rough、estimate_request_tokens_rough。纯函数。

### Step 3.4: Context length 解析链

Config → 本地缓存 → Provider 查询 → models.dev → 默认表 → fallback。引入 reqwest。

### Gate 3
- [ ] Provider prefix 边界用例全覆盖
- [ ] Token 估算与 Python 输出交叉验证（10+ 样本）

---

## Phase 4: Memory 系统（hermes-memory）

**目标**：MemoryProvider trait + MemoryManager 编排。

**Rust 概念**：trait, dyn Trait, Box<dyn Trait>, 默认方法, Send + Sync

### Step 4.1: MemoryProvider trait

从 Python ABC 翻译。每个 abstractmethod → required 方法，有默认体的 → default 实现。写 NoopMemoryProvider。

**对照**：`/Users/rongshen/github/hermes-agent/agent/memory_provider.py`
**参考**：`/Users/rongshen/github/moltis/crates/service-traits/src/lib.rs`（Noop 模式）

### Step 4.2: MemoryManager 编排器

add_provider（限制 1 个外部），build_system_prompt，prefetch_all，sync_all，错误隔离。

**对照**：`/Users/rongshen/github/hermes-agent/agent/memory_manager.py`

### Step 4.3: 上下文栅栏辅助函数

sanitize_context, build_memory_context_block。

### Step 4.4: 契约测试

注册 → 工具出现，外部 provider 限制，路由正确性，失败隔离。

### Gate 4
- [ ] 契约测试全过
- [ ] 1 builtin + 1 external 正常，第 2 个 external 被拒绝

---

## Phase 5: Tool Registry（hermes-tools）

**目标**：工具注册和分发系统。

**Rust 概念**：HashMap 所有权, Arc, Box<dyn Fn>, 线程安全共享状态

### Step 5.1: AgentTool trait + ToolRegistry struct

register / deregister / get_definitions / dispatch。

**参考**：`/Users/rongshen/github/moltis/crates/agents/src/tool_registry.rs`

### Step 5.2: tool_error / tool_result 辅助函数

### Step 5.3: Toolset 可用性检查

### Gate 5
- [ ] 注册 → 分发 → 验证结果
- [ ] 注销 → 分发返回错误

---

## Phase 6: Prompt Builder + Context Compressor（hermes-prompt, hermes-compressor）

**目标**：系统提示词组装和上下文压缩。

**Rust 概念**：Path/PathBuf, 文件 I/O, Iterator, builder 模式, Cow

### Step 6.1: 注入检测扫描器

威胁模式匹配 + 不可见 Unicode 检测。纯字符串处理。

**对照**：`/Users/rongshen/github/hermes-agent/agent/prompt_builder.py`（lines 36-73）

### Step 6.2: Context file 发现

git root 遍历，.hermes.md → AGENTS.md → CLAUDE.md → .cursorrules 优先链。

### Step 6.3: Skills 和身份提示词组装

load_soul_md, build_skills_system_prompt, platform hints。

### Step 6.4: Context compressor — 工具输出裁剪

替换旧工具结果为占位符。无 LLM 调用。

### Step 6.5: Context compressor — LLM 摘要

Token 预算计算，头尾保护，结构化摘要模板，辅助 LLM 调用（trait 化以支持 mock）。

### Gate 6
- [ ] 注入扫描器捕获所有已知威胁模式
- [ ] Context file 发现在测试目录结构中正确工作
- [ ] Compressor 在 mock LLM 下正确裁剪和摘要

---

## Phase 7: LLM Provider 抽象（hermes-providers）

**目标**：LlmProvider trait + OpenAI / Anthropic 实现。

**Rust 概念**：async/await, tokio, Stream, Pin<Box<dyn Stream>>, reqwest streaming, SSE

### Step 7.1: LlmProvider trait

complete / stream / supports_tools / context_window。CompletionResponse / Usage / StreamEvent 类型。

**参考**：`/Users/rongshen/github/moltis/crates/agents/src/model.rs`（line 356+）

### Step 7.2: OpenAI-compatible provider（chat_completions）

/v1/chat/completions endpoint。覆盖 OpenRouter / 本地模型。

### Step 7.3: Anthropic Messages provider

Messages API + prompt caching markers。

### Step 7.4: Provider 契约测试

mockito HTTP mock，验证 complete / stream / tool calls。

### Gate 7
- [ ] 契约测试全过（mock HTTP）
- [ ] 可选：真实 API 手动验证

---

## Phase 8: Agent Loop（hermes-agent）

**目标**：核心 agent 循环。

**Rust 概念**：复杂状态机, loop/break, tokio::spawn, futures::join_all, Fn trait

### Step 8.1: AgentConfig struct

从 AIAgent.__init__() 提取 ~60 个配置字段。

**对照**：`/Users/rongshen/github/hermes-agent/run_agent.py`（lines 487-570）

### Step 8.2: AgentState struct

运行时可变状态：messages, session_id, compression_count, turn_count, token 累加器。

### Step 8.3: 单轮执行

发消息 → 收响应 → 检测 tool calls → 执行工具 → 追加结果。核心循环体。

**参考**：`/Users/rongshen/github/moltis/crates/agents/src/runner.rs`（line 782+）

### Step 8.4: 并行工具执行

多 tool call 并发执行，tokio::spawn + join_all。

### Step 8.5: Context window 管理

将 compressor 接入循环，LLM 调用前检查 token → 触发压缩。

### Gate 8
- [ ] Mock provider 下完成多轮对话
- [ ] Tool calls 正确执行并反馈
- [ ] 压缩在阈值超出时触发
- [ ] Token 统计与 Python 相同对话匹配

---

## Phase 9: PyO3 Bridge（hermes-pyo3）

**目标**：通过 PyO3 暴露 Rust 内核给现有 Python 代码。

**Rust 概念**：PyO3 #[pyclass], #[pymethods], GIL, maturin, Python 互操作

### Step 9.1: PyO3 模块骨架 + maturin 配置

### Step 9.2: 暴露 SessionDb

#[pyclass] 包装，Python::allow_threads 释放 GIL。

### Step 9.3: 暴露 MemoryManager

### Step 9.4: 暴露 agent loop（run_conversation）

### Step 9.5: 与现有 Python CLI 集成测试

import hermes_rs → 创建 session → 运行对话 → 验证输出与纯 Python 路径一致。

### Gate 9
- [ ] `maturin develop` 成功
- [ ] `import hermes_rs` 在 Python 中工作
- [ ] 性能基准：Rust SessionDb vs Python SessionDB（10K 消息 insert/search）
- [ ] 现有 `cli.py` 在导入 hermes_rs 后正常工作

---

## Phase 10: 加固和切换

### Step 10.1: tracing 结构化日志
### Step 10.2: 错误上下文丰富
### Step 10.3: cargo doc 文档
### Step 10.4: Feature flag 灰度（HERMES_USE_RUST=1）
### Step 10.5: 弃用路径

### Gate 10（最终）
- [ ] cargo doc 无警告
- [ ] 所有公开 API 有文档
- [ ] Feature flag 在 CLI 和 gateway 上测试通过
- [ ] Rust 产生的 SQLite DB 与 Python 读写双向兼容

---

## 质量规则（贯穿所有 Phase）

1. **每步不超过 ~500 行新代码**，必须逐行可审查
2. **测试先行**：先写测试定义契约，再写实现
3. **每个 Phase 完成后 `just gate` 必须通过**：build + clippy -D warnings + fmt --check + test
4. **交叉验证**：数据层的每个模块必须与 Python 实现产生相同输出
5. **不跳步**：必须按 Phase 顺序执行，前一个 Gate 未过不开始下一个

## Moltis 参考索引

| 模式 | 参考文件 |
|------|----------|
| Workspace lints | `/Users/rongshen/github/moltis/Cargo.toml` (lines 124-137) |
| 错误处理 | `/Users/rongshen/github/moltis/crates/common/src/error.rs` |
| Service traits + Noop | `/Users/rongshen/github/moltis/crates/service-traits/src/lib.rs` |
| AgentTool trait | `/Users/rongshen/github/moltis/crates/agents/src/tool_registry.rs` |
| LlmProvider trait | `/Users/rongshen/github/moltis/crates/agents/src/model.rs` (line 356) |
| Message 类型 | `/Users/rongshen/github/moltis/crates/sessions/src/message.rs` |
| Agent runner | `/Users/rongshen/github/moltis/crates/agents/src/runner.rs` (line 782) |
| rustfmt/clippy 配置 | `/Users/rongshen/github/moltis/rustfmt.toml`, `clippy.toml` |

## Rust 概念学习进度

| Phase | 新概念 |
|-------|--------|
| 0 | Cargo workspace, editions, lints |
| 1 | struct, enum, derive, serde, Option, Vec, 单元测试 |
| 2 | sqlx (async SQLite), async/await 基础, Mutex, 闭包, ? 操作符, tempfile |
| 3 | HashMap, regex, OnceLock, reqwest, Result/Option 链 |
| 4 | trait, dyn Trait, Box<dyn Trait>, Send + Sync |
| 5 | Arc, 线程安全共享状态 |
| 6 | Path/PathBuf, 文件 I/O, Iterator, Cow |
| 7 | async/await, tokio, Stream, Pin<Box<dyn Stream>> |
| 8 | 状态机, tokio::spawn, join_all |
| 9 | PyO3, GIL, maturin |
