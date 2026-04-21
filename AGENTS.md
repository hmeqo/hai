# AGENTS.md

## 项目概述

`hai` 是一个 Telegram bot agent，基于 `autoagents` 框架，使用 PostgreSQL（含 `pgvector` 扩展）。单 crate 工作区：`crates/hai`。

## 开发命令

```bash
# 编译检查（不需要 DB 连接，日常用这个）
SQLX_OFFLINE=true cargo check

# 运行
cargo run --bin hai -- run

# 查看当前配置（含人格参数 + 完整 system prompt）
cargo run --bin hai -- config
```

## 数据库

- 连接字符串在 `.env` 的 `DATABASE_URL`，默认 `postgres://postgres:@localhost:5433/hai`
- 需要启用 `pgvector` 扩展
- 迁移：`cargo sqlx migrate run`
- **新增或修改 `sqlx::query!` / `sqlx::query_as!` 后，必须重跑 `cargo sqlx prepare --workspace`**，否则 `SQLX_OFFLINE=true` 下编译失败
- 没有 down migration，回滚需手动执行 SQL
- LSP 报 `error communicating with database` 是正常现象（离线模式），不是代码错误
- `TIMESTAMPTZ` 字段需显式标注：`updated_at as "updated_at!: jiff_sqlx::Timestamp"`

## 架构

```
main → Coordinator::run()
  ├── BotHandler        # 接收 Telegram 消息，发出 AgentEvent
  ├── AgentHandler      # 消费 AgentEvent，调用 LLM，发出 BotSignal
  └── TelegramSender    # 消费 BotSignal，调用 Telegram API
```

**层次结构**（不要跨层依赖）：

- `domain/entity/` — 纯数据结构
- `domain/repo/` — SQL 查询，只依赖 domain
- `domain/service/` — 业务逻辑，依赖 repo
- `agent/` — LLM / 提示词 / 工具，依赖 service

## 关键设计约定

**Agent 输出结构化**：`MainAgent::Output = AgentOutput`（非 `String`）。LLM 通过 `output_schema` 约束在 Final Response 输出 JSON，`execute()` 解析后持久化 scratchpad。解析失败 fallback 到 `Default`，不 panic。

**Scratchpad**：与 chat 一对一（`PRIMARY KEY (chat_id)`），每次 agent 运行结束自动覆盖。`token_count` 在 `ScratchpadService::save()` 计算。

**System Prompt 四层**（组装顺序即优先级）：
1. `personality_context()` — 身份 + 说话基调 + 性格画像
2. scene prompt — 群聊/私聊行为模式（`group_prompt` / `private_prompt`，可在 config 覆盖）
3. `TOOL_MANUAL` — 工具手册
4. `agent.system_prompt` + Skills — 用户自定义叠加层

任务指令在动态 Task Message 里，不在 system prompt。

**AgentEvents trait**：对 `&[AgentEvent]` 的语义查询（`has_private()`、`all_interruptible()`、`causes()`）统一通过此 trait，不要在调用处写 `iter().any(|e| matches!(...))`。

**BotSignal::Typing**：仅在 `TriggerCause::Private` 时发送，由 `notify_typing()` 封装。`TelegramSender` 通过 `resolve_platform_chat_id()` 统一解析内部 `chat_id` → Telegram 外部 id。

## 人格系统

`PersonalityConfig`（`config/schema.rs`）的维度：`sociability`、`verbosity`、`honesty`、`humor`、`empathy`、`mood`（均 0.0–1.0）+ `communication_style`（字符串）+ `interests`。

- `sociability` 同时控制 prompt 和行为参数（`min_heat`、`conversation_window_secs`），改它会影响触发频率
- 性格画像由 `agent/core/prompts.rs` 的 `build_character_sketch()` 生成，**不含数值参照表**（已移除）
- `communication_style` 是说话基调，放在身份声明紧后方，优先级高于维度叙事
- 开口动机（求助无人答/信息有误才插话）作为固定底色内化在画像叙事里，不是可调维度
- 人格参数可在 `.hai/config.toml` 的 `[agent.personality]` 覆盖，无需改代码
