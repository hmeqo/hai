# AGENTS.md

## 开发命令

```bash
cargo check                          # 日常编译
cargo run --bin hai                  # 启动 bot
cargo run --bin hai -- config        # 查看当前配置
cargo run --bin hai -- config --format toml  # TOML 格式输出
cargo sqlx migrate run               # 跑迁移（无 down migration）
cargo sqlx prepare --workspace       # 修改 sqlx::query! 后必须重跑
```

## 编码规范

- `rustfmt.toml`: `imports_granularity = "Crate"`
- LSP 报 `error communicating with database` 是离线模式的正常现象，忽略即可
- 无 CI / 无 pre-commit
- 层次依赖：`entity → vo → repo → service → agent → app`，`infra` 不依赖上层；`bot` 依赖 `app`，不依赖 `agent`
- 多 chat 并行，单 chat 串行

## 信号通信架构

Bot 和 Agent 之间通过 `BotLink` / `AgentLink` 双向 channel 通信，**连接本身承载路由身份**：

```
BotA ──BotLink──► AgentGateway ──BotConn──► BotA
       event_tx      │       signal_tx
                     ▼
               ChatSession 持有 BotConn
               RoundContext 持有 BotConn
```

- `BotLink`（bot 侧）：持有 `event_tx`（发事件给 agent）+ `signal_rx`（收 agent 信号）
- `AgentLink`（agent 侧）：持有 `event_rx` + `signal_tx`
- `BotConn`：封装 `signal_tx` + `BotProfile`，提供类型安全方法
- `BotProfile`：平台无关的身份信息（`account_id` / `username` / `name`）

## 核心结构职责

| 结构 | 职责 |
|------|------|
| `AgentCtx` | LLM 管理、Agent 组装、任务执行（Arc 共享） |
| `AgentGateway` | 连接管理（add_connection）、事件路由到 session |
| `ChatSession` | 单 chat 防抖 + 任务调度 |
| `ActiveTask` | JoinSet 包装，支持中断 |
| `RoundContext` | 单次 agent 触发上下文（chat_id + conn + events） |
| `TelegramPlatform` | Telegram 平台适配器（dispatcher + signal handler） |

## Bot 配置格式

```toml
[bot.main]
type = "telegram"
bot-token = "xxx"
allowed-chat-ids = [123456]

[bot.dev]                     # 省略 type 时从 key 推断
bot-token = "yyy"
allowed-chat-ids = []
```

## 注意事项

- 改 `sqlx::query!` 后必须先 `cargo sqlx prepare --workspace`，否则编译报错
- `TIMESTAMPTZ` 字段需显式标注：`updated_at as "updated_at!: jiff_sqlx::Timestamp"`
- `Scratchpad` 与 chat 一对一（`PRIMARY KEY (chat_id)`），每次 agent 运行结束覆盖
- `PerceptionService::upsert()` 内部自动生成并保存 embedding，调用方只需调一次
- `AgentEvents` trait（`agent/event/cause.rs`）聚合语义查询，**不要**在调用处手写 `iter().any(...)`
- Config 覆盖链：`.hai/config.toml` → 环境变量 `HAI_` 前缀覆盖 → 运行时热加载
- `HAI_LOCAL_MODE=1` 强制使用 `.hai/`，否则回退 `$XDG_CONFIG_HOME/hai/`
