# AGENTS.md

## 开发命令

```bash
cargo check                          # 日常编译（离线模式 SQLX_OFFLINE=true）
cargo run --bin hai                  # 启动 bot
cargo run --bin hai -- config        # 查看当前配置
cargo run --bin hai -- config --format toml  # TOML 格式输出
cargo sqlx migrate run               # 跑迁移（无 down migration）
cargo sqlx prepare --workspace       # 改 sqlx::query! 后必须重跑
```

## 架构

```
crates/hai/src/
├── agent/        LLM 会话管理、工具、角色系统（无平台依赖）
│   ├── link.rs   ← PlatformHandler trait（agent 与 platform 的契约）
│   └── tools/    工具（如 analyze_attachment → 委托给 handler）
├── platform/     平台实现（每平台一个子目录）
│   ├── manager.rs  启动所有 bot 实例，注册到 AgentGateway
│   └── telegram/   Telegram 实现
│       ├── actor.rs     kameo actor（消息发送 API + 入库）
│       ├── handler.rs   实现 PlatformHandler（薄胶水层）
│       ├── media.rs     文件缓存 + 附件解析 + multimodal 分发
│       ├── dispatcher.rs 事件循环
│       └── service.rs   TelegramService（原始文件下载/URL）
├── app/          上下文组装、配置热加载
├── domain/       实体、仓库、服务（DB 访问）
├── config/       配置 schema、CLI 参数
├── infra/        基础设施（cache 等）
├── agentcore/    LLM provider 封装（autoagents 适配）
└── error.rs      错误类型
```

依赖方向：`entity → vo → repo → service → agent → app`，`infra` 不依赖上层；`platform` 依赖 `app`，不依赖 `agent`。

## 平台抽象

- `agent/link.rs` 定义 `PlatformHandler` trait：发消息、发语音、打字指示、文件下载/URL、附件分析
- 新平台只需实现 `PlatformHandler`，放置于 `platform/<name>/`
- `BotConn` 持有 `Arc<dyn PlatformHandler>`，agent 层零平台依赖

## 通信架构

```
Bot ──BotLink──► AgentGateway ──BotConn──► Bot
     event_tx      │       handler
                   ▼
             ChatSession → RoundContext → ToolT
```

- `BotLink`: bot 侧连接半体（`event_tx` + `signal_rx`）
- `AgentLink`: agent 侧连接半体（`event_rx` + `signal_tx`）
- `BotConn`: 封装 `signal_tx` + `Arc<dyn PlatformHandler>`
- 多 chat 并行，单 chat 串行

## 关键约定

- **kameo actor**：`TelegramBotActor` 处理消息发送 + 入库。外部不直接调 actor，通过 `TelegramPlatformHandler`（`handler.rs`）委托。`TelegramMediaAnalyzer`（`media.rs`）处理附件分析。
- **sqlx 离线模式**：LSP 报 `error communicating with database` 是离线模式的正常现象，忽略即可
- `sqlx::query!` 改后必须 `cargo sqlx prepare --workspace`
- `TIMESTAMPTZ` 字段需显式标注：`updated_at as "updated_at!: jiff_sqlx::Timestamp"`
- `AgentEvents` trait（`agent/event/cause.rs`）聚合语义查询，不要手写 `iter().any(...)`
- Config 覆盖链：`.hai/config.toml` → `HAI_` 环境变量 → 运行时热加载
- `HAI_LOCAL_MODE=1` 强制使用 `.hai/`，否则回退 `$XDG_CONFIG_HOME/hai/`

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

## 编码

- `rustfmt.toml`: `imports_granularity = "Crate"`, `group_imports = "StdExternalCrate"`
- nightly toolchain
- 无 CI / 无 pre-commit
- `cargo run --bin hai -- run` 启动
- edition 2024
