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
- JSONB 列用 `toasty::Json<T>` 包装
- `jiff::Timestamp` 原生支持（toasty jiff feature）
- 类型安全 PK：`domain/vo/id.rs`（`ChatId`、`MessageId` 等），模型字段用裸类型保持 ORM 兼容

## 架构

```
hai/src/
├── agent/               agent 业务逻辑
│   ├── node/               agent 节点定义（每个类型一个目录）
│   │   └── main/           MainAgent 节点（SystemPromptBuilder + build_react_config）
│   ├── runtime/            共享：AgentEngine + AgentSession + ReactLoop
│   │   ├── context.rs       RunContext + ToolContext
│   │   ├── types.rs         Run / RunOutput / ToolCallResult 类型
│   │   ├── react.rs         run_react_loop（纯函数，node 无关）
│   │   ├── engine.rs        AgentEngine（共享单例）
│   │   ├── registry.rs      SessionManager（background mpsc 自动清理）
│   │   ├── shell.rs         ShellRuntime（沙箱 shell）
│   │   ├── event/           WakeEvent / WakeReason（跨层事件类型）
│   │   │   └── wake.rs
│   │   └── session/         AgentSession（状态机 + 调度器）
│   │       ├── mod.rs        AgentSession + ActiveRun + event loop
│   │       ├── proxy.rs      SessionHandle
│   │       ├── dispatch.rs   事件调度 + 派发
│   │       ├── run.rs        spawn_run_task + assemble_run
│   │       ├── messages.rs   消息收集
│   │       ├── context.rs    build_run_context 工厂
│   │       ├── scheduler.rs  EventScheduler（热度 + 窗口 + 防抖 + 重试）
│   │       └── attention.rs  Heat + Window（调度器数学原语）
│   ├── tools/              工具实现（每个文件一个工具集）
│   ├── context/            提示词上下文渲染（XML → prompt string）
│   ├── link.rs             BotHandle + PlatformHandler trait
│   ├── personality/        性格配置渲染
│   └── multimodal/         多媒体服务
├── agentcore/           基础设施层（不依赖 agent/domain）
│   ├── tool.rs          AgentTool trait + ToolError + 辅助函数
│   ├── mcp.rs           McpManager + McpServerHandle（基于 rmcp）
│   ├── embedding.rs     EmbeddingService trait
│   ├── provider.rs      ProviderBackend + genai Client 工厂
│   ├── skills/          SkillManager
│   ├── render/          渲染引擎（XML/JSON/MD）
│   └── token.rs         计数
├── util/                工具函数
│   └── pgvector.rs      pgvector 搜索/写入封装
├── domain/              领域层（model + service + vo）
│   ├── service/         直接调 toasty ORM + sqlx（pgvector 查询）
│   └── model/           toasty 模型定义（无 embedding 字段，由 rebuild 管理）
├── platform/            Telegram 平台集成
│   └── telegram/
│       ├── dispatcher.rs      teloxide 路由（薄层）
│       └── message_handler.rs  消息处理（账号解析 + 持久化 + 事件分发）
├── config/              配置系统
└── app/                 应用上下文 + 启动
```

## 通信模型

```
Dispatcher ──handle.wake(WakeEvent)──► SessionHandle.wake_tx
                                            │
                                      AgentSession::run()
                                            │
                                 match state { Idle, Active(ActiveRun) }
                                 Idle → poll_idle() → select! { wake_rx, status_rx, deadline }
                                 Active → active.poll() { wake_rx, status_rx, result_rx }
                                            │
                                      try_dispatch() → dispatch_with()
                                                         │ (tokio::spawn + oneshot)
                                                     node.run()
                                                         │
                                                     Run ◄──────── result_rx
                                            │
                                      on_run_complete() → self.runs.push + schedule.refresh
```

- **Platform → agent**：dispatcher → `registry.get_or_create(ChatId)` → `handle.wake(WakeEvent)`
- **Agent run 执行**：`dispatch_with()` → `spawn_run_task()`（自由函数）→ Result 通过 per-run oneshot 返回
- **状态查询**：`handle.status()`（mpsc 通道）
- **内部命令**走 `self.bot.send_message()`，不经会话

## Session 生命周期

- `SessionManager` 在 `agent/runtime/registry.rs`，管理所有 agent session 的创建与清理
- `get_or_create(ChatId)` → 读锁快速查找，写锁 double-check 后插入
- 后台 task 监听 `mpsc::UnboundedReceiver<ChatId>`，收到退出信号后 `is_finished()` 确认 → 自动移除
- session task 退出时通过 `cleanup_tx.send(chat_id)` 通知清理，无需 `sweep()` 轮询
- Session 退出后如果同 chat 又来消息，会在 `get_or_create` 创建新 session
- 所有 session 共用 `AgentEngine`（LLM client + MCP + skills），通过 `Arc` 共享

## 调度策略

| 方法 | 含义 |
|------|------|
| `is_addressed()` | Direct / Mention → 刷新窗口 + 热量 |
| `is_rapid()` | Scheduled / Command → 绕过防抖 + 窗口 + 热度 |
| `is_mergeable()` | Observe / Mention / Direct → 同轮次可合并 |
| debounce 0.5s | 最后一次事件后等 500ms 才 dispatch |
| Guard window 3s | 3s 内新 addressed 事件可 interrupt 当前 round |

## Tools 层

- 定义在 `agentcore::tool`：`AgentTool` trait（name / description / schema / execute）
- 工具实现放 `agent/tools/`，每个模块一个 `pub fn tools(ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>>` 工厂
- 无 `ToolBridge` / 无 `ToolT` / 无 autoagents — 全部用本地 `AgentTool` trait
- 大部分工具用 `#[hai_macros::tool]` 宏自动生成 `impl AgentTool`（name 从 struct 名推导，description 从 doc comment 取，schema 从 `schemars::schema_for!` 生成，execute 反序列化后委托给 `self.exec(typed)`）
- RunShell / AnalyzeAttachment 因动态 description 保留手动 `impl AgentTool`
- `chat_id` 来自 struct 字段（round 创建时注入）
- `#[serde(deserialize_with = "deserialize_option_lenient_u64")]` 处理字符串/数字混用
- args struct 用 `#[derive(Deserialize, JsonSchema)]` + doc comments → `schemars` 自动生成 JSON Schema
- 辅助函数：`tool_ok()` / `tool_data()` / `tool_err()` / `MapToolErr`（在 `agentcore::tool`）

## Tools 工厂

```rust
pub fn tools(ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    vec![Arc::new(MyTool { field: ctx.some_field.clone() })]
}
```

`get_main_agent_tools()` 在 `tools/mod.rs` 中合并所有模块的工具列表，`spawn_run_task()` 再与 MCP 工具合并后传入 `ReactLoop`。

## MCP

- 基于 `rmcp`（官方 MCP Rust SDK）
- `McpManager` 在启动时加载所有 MCP server 配置，每个 server 对应一个 `McpServerHandle`
- 子进程 stderr 被 pipe 到 `tracing::debug!(target: "hai::mcp")`，由 `[logging] level` 控制显隐
- rmcp 内部日志被全局 EnvFilter 限制到 `warn`（在 `app/mod.rs` 设置）

## Embedding

- `agentcore::embedding::EmbeddingService` trait（`generate_embedding(&self, text) -> Result<Vec<f32>>`）
- `MultimodalService` 实现该 trait
- `domain/service/` 通过 `Arc<dyn EmbeddingService>` 注入，不直接依赖 agent 层
- 向量存储在 PostgreSQL `embedding vector(N)` 列中，由 `pgvector` 扩展管理
- 搜索走 `util::pgvector`（`search_embedding_vec` 用 `<->` 余弦距离 + IVFFlat 索引）
- 业务层搜索：sqlx 查 `(id, distance)` → toasty `in_list` 加载完整对象
- `hai rebuild embeddings` 读取 `[multimodal.embedding.dimension]`，自动 `ALTER COLUMN TYPE vector(N)` + 重算 + 建 IVFFlat 索引
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
