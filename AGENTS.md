## 开发命令

```bash
cargo check                       # 编译
cargo clippy --all-targets        # lint
cargo run --bin hai               # 启动
cargo run --bin hai -- config     # 查看配置
cargo run -- db create            # 创建数据库
cargo run -- db migrate           # 执行迁移
cargo run -- db rebuild embeddings   # 用当前 embedding model 重算所有向量
cargo run --bin toasty-cli -- migration generate    # 生成 migration
cargo run --bin toasty-cli -- migration apply       # 执行迁移
```

- ORM: `toasty`（tokio 团队，0.7），模型定义在 `domain/model/`
- schema 由 `toasty-cli` 管理：`migration generate` → 审查 SQL → `migration apply`
- `db.push_schema()` 已移除，表结构完全由 toasty 迁移控制
- JSONB 列用 `toasty::Json<T>` 包装
- `jiff::Timestamp` 原生支持（toasty jiff feature）
- 类型安全 PK：`domain/vo/id.rs`（`ChatId`、`MessageId` 等），模型字段用裸类型保持 ORM 兼容

## 架构

```
hai/src/agent/runtime/session/
├── mod.rs       状态机（Idle/Running 两阶段 + 事件循环）
├── proxy.rs     ChatSessionHandle + spawn_chat_session + HeartbeatTask
├── dispatch.rs  事件入队 + 调度 dispatch
├── round.rs     轮次组装 + 完成回调 + spawn_round_task（自由函数）
├── messages.rs  消息获取（首轮 get_context_messages + 后续 get_messages_window）
└── context.rs   RoundContext 工厂（从 events 构建执行上下文）

hai/src/agent/runtime/
├── event/
│   ├── scheduler.rs   EventScheduler（batch + heat + window + debounce 0.5s + 过期）
│   ├── wake.rs        WakeEvent（Arc<WakeEventInner>）+ WakeReason
│   └── attention.rs   Heat + Window（内部实现）
├── round.rs           Round + RoundTaskPayload（纯数据，含 shown_*_ids 去重字段）
├── ctx.rs             RoundContext
├── engine.rs          AgentEngine（LLM + agent 组装）
├── shell.rs           ContainerGuard（RAII，Drop 自动 docker rm -f）
└── registry.rs        ChatSessionManager（管理 ChatSessionHandle）
```

```
domain/
├── model/          #[derive(toasty::Model)] 模型定义
├── service/        业务逻辑，直接调 toasty ORM（无 repo 层）
├── vo/             值对象（ChatId、MemoryInput、Source 等）
└── db.rs           init_db() → toasty::Db
```

## 通信模型

```
Dispatcher ──handle.wake(WakeEvent)──► ChatSessionHandle.wake_tx
                                           │
                                     SessionLoop::run()
                                           │
                                Idle: select! { wake_rx, status_rx, deadline }
                                Running: RunningRound::poll() { wake_rx, status_rx, result_rx }
                                           │
                                     dispatch_with() → assemble_round() + spawn_round_task()
                                                         │ (tokio::spawn + oneshot)
                                                     engine.run()
                                                         │
                                                     Round ◄─────── result_rx
                                           │
                                     on_round_complete() → self.rounds.push + schedule.refresh
```

- **Platform → agent**：dispatcher → `registry.get_or_create(ChatId)` → `handle.wake(WakeEvent)`
- **Agent round 执行**：`dispatch_with()` → `spawn_round_task()`（自由函数）→ Result 通过 per-round oneshot 返回
- **状态查询**：`handle.status()`（mpsc 通道）
- **内部命令**走 `self.bot.send_message()`，不经会话
- 无 kameo actor 框架，纯 tokio mpsc + oneshot + select!

## 调度策略

| 方法 | 含义 |
|------|------|
| `is_addressed()` | Direct / Mention → 刷新窗口 + 热量 |
| `is_rapid()` | Scheduled / Command → 绕过防抖 + 窗口 + 热度 |
| `is_mergeable()` | Observe / Mention / Direct → 同轮次可合并 |
| debounce 0.5s | 最后一次事件后等 500ms 才 dispatch |
| Guard window 3s | 3s 内新 addressed 事件可 interrupt 当前 round |

## Tools 层

- `chat_id` 来自 `self.chat_id`（`struct` 字段，不从 LLM 参数获取）
- `#[serde(deserialize_with = "deserialize_option_lenient_u64")]` 处理字符串/数字混用
- 所有 tool 错误统一用 `tool_ok()` / `tool_data()` / `tool_err()` / `MapToolErr`
- `pub fn tools(ctx: &RoundContext) -> Vec<Arc<dyn ToolT>>` 工厂模式

## Embedding

- JSONB 列存 `Vec<f32>` 序列化数组，无固定维度
- `util::vector::cosine_similarity()` + `util::vector::embedding_from_json()`
- `hai rebuild embeddings` 全量重生成（遍历 memory / topic / perception）
- `search_related_dedup()` 封装去重 + 2/3 缩减

## Bot 配置

```toml
[bot.telegram]
bot-token = "xxx"
allowed-chat-ids = [123456]
```

省略 `type` 时从 key 名推断（`telegram` / `tg` → Telegram）。
Config 覆盖链：`.hai/config.toml` → `HAI_` 环境变量 → 运行时热加载。
`HAI_LOCAL_MODE=1` 强制使用 `.hai/`，否则回退 `$XDG_CONFIG_HOME/hai/`。

## Provider

- `api_key` 是 `Option<String>`，Ollama 等本地服务可省略
- 已知 backend 需要显式注册 `[providers.*]`，不自动注册

## 编码

- `imports_granularity = "Crate"`, `group_imports = "StdExternalCrate"`
- nightly toolchain, edition 2024
- 无 CI / 无 pre-commit
- `pub(super)` 对 `runtime/` 内可见；`pub(crate)` 对 `agent/` 内可见
- 倾向 RAII 封装（`HeartbeatTask`、`ContainerGuard`）和语义封装（`search_related_dedup`）
- **Service** 直接调 toasty ORM，无 repo 层
