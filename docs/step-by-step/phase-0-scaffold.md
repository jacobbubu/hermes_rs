# Phase 0: Workspace 脚手架

**状态**: 已完成 (2026-04-08)
**Issue**: #1
**Commit**: `4d98c86`

## 目标

搭建一个空的、能编译的 Rust workspace，CI 基础设施就位。

## 做了什么

### Step 0.1: Workspace 根 Cargo.toml

创建 workspace 配置，核心决策：

- `edition = "2024"`，`rust-version = "1.85"`
- **Workspace lints**（参考 Moltis）：
  - `unsafe_code = "deny"` — 禁止 unsafe 代码
  - `unwrap_used = "deny"` — 禁止 `.unwrap()`，强制用 `?` 或 `.ok()` 处理错误
  - `expect_used = "deny"` — 同上，禁止 `.expect()`
- **Workspace dependencies** 预填充：serde, serde_json, thiserror, anyhow, tokio, sqlx, reqwest, uuid, regex, tracing, chrono

### Step 0.2: 第一个空 crate `hermes-types`

```
crates/hermes-types/
  Cargo.toml    # 继承 workspace lints 和 edition
  src/lib.rs    # 仅模块级文档注释
```

每个 crate 的 `Cargo.toml` 通过 `[lints] workspace = true` 继承 workspace 级 lint 规则。

### Step 0.3: 配置文件

| 文件 | 用途 |
|------|------|
| `rustfmt.toml` | 代码格式化规则（仅 stable 选项） |
| `clippy.toml` | Clippy lint 配置 |
| `rust-toolchain.toml` | 固定 stable channel |
| `justfile` | 开发命令：`just gate`（fmt + clippy + test） |

## Rust 概念

这个 Phase 引入的 Rust 概念：

- **Cargo workspace** — 多 crate 项目组织方式，共享依赖版本和 lint 规则
- **Edition** — Rust 语言版本（2024），决定语法和默认行为
- **Workspace lints** — 在根 `Cargo.toml` 定义，所有 crate 继承
- **rustfmt / clippy** — 格式化器和静态分析器，Rust 生态的标配

## 验证

```bash
cargo fmt --check   # 格式检查
cargo clippy --workspace --all-targets -- -D warnings  # lint 检查
cargo test --workspace  # 运行测试（此时 0 个测试）
```

## 目录结构

```
hermes_rs/
├── Cargo.toml          # workspace 根
├── Cargo.lock
├── clippy.toml
├── rustfmt.toml
├── rust-toolchain.toml
├── justfile
├── LICENSE
├── .gitignore
└── crates/
    └── hermes-types/
        ├── Cargo.toml
        └── src/
            └── lib.rs
```
