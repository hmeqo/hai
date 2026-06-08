# AGENTS.md

## 开发命令

```bash
cargo check                       # 编译（离线 SQLX_OFFLINE=true）
cargo clippy --all-targets        # lint
cargo test --no-run               # 编译测试
cargo sqlx prepare --workspace    # 改 sqlx::query! 后必须重跑
cargo run --bin hai               # 启动
cargo run --bin hai -- config     # 查看配置
```

- LSP 报 `error communicating with database` 是 sqlx 离线模式正常现象，忽略
- `TIMESTAMPTZ` 字段需显式标注：`updated_at as "updated_at!: jiff_sqlx::Timestamp"`

## 架构要点

```
hai/src/
├── agent/           LLM 会话管理、工具（无平台依赖）
│   ├── link.rs      PlatformHandler trait + BotHandle
│   ├── runtime/
│   │   ├── actor.rs        ChatActor (kameo actor, 事件调度 + round 生命周期 + 容器管理)
│   │   ├── engine.rs       AgentEngine (LLM + ChatActorManager)
│   │   ├── event/          EventScheduler (batch + heat + window)
│   │   ├── container.rs    容器生命周期（docker/podman create / exec / destroy）
│   │   ├── rounds.rs       RoundManager（round 队列 + 当前 task）
│   │   ├── ctx.rs          RoundCtx
│   │   ├── data.rs         Round, ToolResult
│   │   ├── query.rs        RoundResult, SchedulerStatus
│   │   ├── registry.rs     ChatActorManager（管理所有 ChatActor）
│   │   └── round_task.rs / task_payload.rs
│   ├── context/             prompt 构建（builder + sections + types）
│   ├── node/output.rs       MainAgentOutput { notes: Option<String> }
│   └── tools/               send_message, send_voice, run_shell, analyze_attachment 等
├── ext/kameo.rs      KameoExt trait: actor.tell(msg).fire()
├── platform/telegram/       平台适配
│   ├── actor.rs      TelegramBotActor (#[derive(Actor)])
│   ├── handler.rs    实现 PlatformHandler
│   └── dispatcher.rs 事件入口（通过 registry.get_or_create 获取 ChatActor）
├── domain/vo/chat_id.rs      ChatId newtype（防止与平台侧 ID 混淆）
└── agentcore/        LLM provider 封装 (autoagents)

依赖: entity → vo → repo → service → agent → app。infra 不依赖上层。platform 依赖 app，不依赖 agent。
```

## 通信模型

```
Dispatcher ──tell(WakeEvent)──► registry.get_or_create(ChatId)
                                     │
                                ChatActor.handle(WaveEvent)
                                     │
                                     ├── scheduler.push(event)
                                     └── try_consume() → RoundManager::spawn()
                                                            │
                                                       engine.run() → MainAgentOutput
                                                            │
                                                       tell(RoundResult) ──► ChatActor
                                                                              │
                                                                           try_consume()
```

- **Agent → platform**：agent tool 调 `bot_actor.ask(SendMessageReq)`（经过 `BotHandle` → `PlatformHandler`）
- **Platform → agent**：dispatcher 调 `engine.sessions.get_or_create(ChatId, spawn_fn)` → `actor.tell(WakeEvent)`
- **查询**：`actor.ask(GetStatus).await` → `SchedulerStatus`
- **fire-and-forget**：`actor.tell(msg).fire()`（`ext/kameo.rs` `KameoExt`，同步 try_send）
- 内部命令（`/help`、`/status`、`/start`）走 `self.bot.send_message()`，不经 actor

## 调度策略

定义在 `scheduler.rs` 的 `impl WakeReason` 中，不是 `wake.rs`（数据 vs 策略分离）：

| 方法 | 含义 |
|------|------|
| `is_addressed()` | `Direct` / `Mention` → 刷新窗口 + 热量 |
| `is_rapid()` | `Scheduled` / `Command` → 绕过 batch deadline |
| `is_mergeable()` | `Observe` / `Mention` / `Direct` → 同轮次可合并 |

## Bot 配置

```toml
[bot.main]
type = "telegram"
bot-token = "xxx"
allowed-chat-ids = [123456]
```

省略 `type` 时从 key 名推断（`telegram` / `tg` → Telegram）。
Config 覆盖链：`.hai/config.toml` → `HAI_` 环境变量 → 运行时热加载。
`HAI_LOCAL_MODE=1` 强制使用 `.hai/`，否则回退 `$XDG_CONFIG_HOME/hai/`。

## 编码

- `imports_granularity = "Crate"`, `group_imports = "StdExternalCrate"`
- nightly toolchain, edition 2024
- 无 CI / 无 pre-commit
