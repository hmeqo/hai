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
hai/src/agent/runtime/
├── session.rs      ChatSessionHandle + SessionLoop（单一状态机：事件循环 + 调度 + rounds 管理）
├── ctx.rs          RoundContext（prompt 构建 + 工具执行共用）
├── engine.rs       AgentEngine（LLM + agent 组装）
├── event/
│   ├── scheduler.rs EventScheduler（batches + heat + window）
│   ├── batch.rs     EventBatch + debounce（0.5s 防抖）
│   ├── wake.rs      WakeEvent + WakeReason
│   └── attention.rs Heat + Window
├── round.rs         Round + RoundTaskPayload（纯数据）
├── shell.rs        ShellRuntime（容器/本地 shell）
└── registry.rs     ChatSessionManager（管理所有 ChatSessionHandle）
```

## 通信模型

```
Dispatcher ──handle.wake(WakeEvent)──► ChatSessionHandle.wake_tx
                                          │
                                     SessionLoop::run()
                                          │
                                     select! { wake_rx, result_rx }
                                          │
                                     try_dispatch() → engine.run() (tokio::spawn)
                                          │
                                     result_tx.send(RoundResult) ◄───────┘
                                          │
                                     on_result() → rounds.push / schedule.refresh
```

- **Platform → agent**：dispatcher 调 `registry.get_or_create(ChatId)` → `handle.wake(WakeEvent)`
- **Agent round 执行**：`try_dispatch()` → `engine.run()` → Result 通过 `result_tx` 返回
- **查询状态**：`handle.status()`（Arc\<RwLock\<SchedulerStatus\>\> 同步快照）
- **内部命令**走 `self.bot.send_message()`，不经会话
- 无 kameo，纯 tokio mpsc + select!

## 调度策略

| 方法 | 含义 |
|------|------|
| `is_addressed()` | `Direct` / `Mention` → 刷新窗口 + 热量 |
| `is_rapid()` | `Scheduled` / `Command` → 绕过防抖+窗口+热度 |
| `is_mergeable()` | `Observe` / `Mention` / `Direct` → 同轮次可合并 |
| `debounce_remaining()` | 0.5s 防抖：最后一次事件后等 500ms 才 dispatch |
| Guard window | 3s 内新 addressed 事件可 interrupt 当前 round |

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
- `pub(super)` 对 `runtime/` 内可见；`pub(crate)` 对 `agent/` 内可见
