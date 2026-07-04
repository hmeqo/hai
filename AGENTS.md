## 开发命令

```bash
cargo check                       # 编译
cargo clippy --all-targets        # lint
cargo run --bin hai               # 启动
cargo run --bin hai -- config     # 查看配置
cargo run -- db create            # 创建数据库
cargo run -- db migrate           # 执行迁移
cargo run -- db rebuild embeddings   # 用当前 embedding model 重算所有向量
cargo run --bin hai -- log           # agent 事件日志（TUI）
cargo run --bin hai -- log --id 42   # 查看单条事件详情
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
│   ├── node/               agent 节点定义
│   │   └── main/           MainAgent（SystemPromptBuilder + ReactLoopConfig）
│   ├── runtime/            共享运行时
│   │   ├── context.rs       RunContext + ToolContext
│   │   ├── types.rs         Messages / Inbox / ProcessingOutput / Turn
│   │   ├── react.rs         ReactRun + run_react_loop（纯函数，node 无关）
│   │   ├── engine.rs        AgentEngine（Arc 共享单例）
│   │   ├── registry.rs      SessionManager（Sessions HashMap + lazy retain）
│   │   ├── shell.rs         ShellRuntime（沙箱 shell）
│   │   ├── event/           WakeEvent + AgentEvent + AgentEventBus
│   │   │   ├── wake.rs      WakeEvent + WakeReason
│   │   │   └── bus.rs       AgentEventPayload（serde tagged enum）+ AgentEventBus
│   │   └── session/         AgentSession（事件循环 + 调度器）
│   │       ├── mod.rs        AgentSession 结构体 + SessionState 枚举
│   │       ├── event_loop.rs  run() 主循环 + idle_tick + ActiveProcessing
│   │       ├── dispatch.rs    dispatch + on_complete + spawn_processing
│   │       ├── prompt.rs      assemble_run + build_run_context + ProcessingPayload
│   │       ├── proxy.rs       SessionHandle + SessionStatus + HeartbeatTask
│   │       ├── conversation.rs Conversation（next_prompt + update）
│   │       ├── scheduler.rs   EventScheduler（pure timing）
│   │       └── attention.rs   Heat + Window（调度器数学原语）
│   ├── tools/              工具实现（每个文件一个工具集）
│   ├── context/            提示词渲染（XML → prompt string）
│   ├── link.rs             BotHandle + PlatformHandler trait
│   ├── personality/        性格配置渲染
│   └── multimodal/         多媒体服务
├── agentcore/           agent 核心库（不依赖 agent/domain）
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
│   ├── model/           toasty 模型
│   │   ├── memory.rs     Memory + MemoryKind（UserFact / Note / Knowledge）
│   │   ├── topic.rs      Topic + TopicStatus
│   │   ├── message.rs    Message
│   │   └── ...
│   ├── service/          业务逻辑（直接调 toasty ORM + sqlx）
│   │   ├── memory.rs     MemoryService（create / update / search / delete）
│   │   └── ...
│   └── vo/              值对象（ChatId, MessageId, TopicSearchResult 等）
├── platform/            Telegram 平台集成
│   └── telegram/
│       ├── dispatcher.rs      teloxide 路由（薄层）
│       └── message_handler.rs  消息处理（账号解析 + 持久化 + 事件分发）
├── config/              配置系统
└── app/                 应用上下文 + 启动
```

## 通信模型

```
Platform → SessionHandle.wake(WakeEvent)
  → Inbox.push(event)
    → idle_tick (Inbox.drain → scheduler.enqueue → scheduler.decide)
      → dispatch(events)
        → spawn_processing → run_react_loop
          → ReactLoopOutput → oneshot → on_complete
            → on_complete → Idle
```

| 层 | 组件 | 通信方式 |
|----|------|---------|
| Platform → Session | `SessionHandle.wake(event)` | `Inbox.push()` + `Notify` |
| Idle → Active | `idle_tick` → `dispatch` | `scheduler.enqueue()` + `scheduler.decide()` |
| Processing 完成 | `oneshot::Receiver` → `on_complete` | `oneshot::channel` |
| 清理 | `on_complete` / Failed / Cancelled | `Inbox.drain()` → `scheduler.enqueue()` → Idle |
| 状态查询 | `SessionHandle.status()` | `mpsc` + `oneshot` 响应 |

## Agent 事件

`AgentEventPayload` 是 serde tagged enum（`#[serde(tag = "event")]`），所有事件统一存在 `event` 表的 `payload` JSONB 列中。

### 事件变体

| 变体 | tag（JSON `event` 字段） | 说明 |
|------|--------------------------|------|
| `SessionCreated` | `session_created` | 会话创建 |
| `WakeStarted` | `wake_started`（别名 `turn_started`） | 一次 processing 启动 |
| `ContextBuilt` | `context_built` | prompt 构建完成 |
| `ToolCall` | `tool_call` | 工具调用 |
| `ToolCallResult` | `tool_call_result` | 工具返回结果 |
| `RunCompleted` | `run_completed` | 一次 run 成功完成 |
| `RunFailed` | `run_failed` | 一次 run 失败 |
| `ModelRetry` | `model_retry` | 模型 retry（`reason` 字段区分 `text_without_tool` / `timeout_retry`） |
| `Preempted` | `preempted` | run 期间 inbox 新事件注入 |
| `SessionDone` | `session_done` | 会话结束 |

- `chat_id` 作为字段嵌入每个变体（不在 DB 列级别独立存储）
- `reason: ModelRetryReason` 是 `#[derive(IntoStaticStr, EnumString)]` 枚举，零分配序列化

## Session 事件流

事件队列统一由 `EventScheduler.queue` 持有，`AgentSession` 不再有 `backlog`。

```
Idle:
  idle_tick(&mut self, inbox: &Inbox):
    select! { notified(), status_rx, deadline }
    inbox.drain() → scheduler.enqueue(events) → scheduler.decide(timeout)
      Ready(events) → dispatch
      Defer → Idle
      Done → 退出

Active:
  on_complete(output, inbox) / Failed / Cancelled:
    inbox.drain() → scheduler.enqueue() → Idle
```

两个关键设计：
- **`scheduler.queue`** 是唯一的挂起事件队列，不在 session 层面重复持有
- 所有事件统一由 `idle_tick` 路径的 `scheduler.decide()` 调度，无捷径

## 调度策略

`scheduler.rs`（pure timing engine）：

| 方法 | 触发条件 | 效果 |
|------|----------|------|
| `is_addressed()` | Direct / Mention | 刷新窗口 + 热量 |
| `is_rapid()` | Scheduled / Command | 绕过 debounce |
| `is_mergeable()` | Observe / Mention / Direct | 同类事件可合并 |
| debounce 0.5s | 最后一次事件后等 500ms | 到达 deadline 才 dispatch |
| Heat spend | `random < heat.value` | 概率性 dispatch（Observe） |

## Memory 系统

```
Memory {
    id: Uuid,
    chat_id: Option<i64>,
    account_id: Option<i64>,   // UserFact 使用
    content: String,
    importance: i32,
    kind: String,               // "user_fact" | "note" | "knowledge"
    meta: Option<Json>,        // 通用元数据（references 等）
    created_at, updated_at: Timestamp,
}

MemoryKind { UserFact, Note, Knowledge }
```

- 所有 Memory 统一嵌入（`needs_embedding()` 已被删除）
- `search_related` 不做 type 排除
- `MemoryService::create(kind, chat_id, content, account_id?, meta?)` 取代旧的 `save_memory(MemoryInput)`
- `MemoryService::update(id, content?, importance?)` 取代 Update 变体

## Tools 层

- 定义在 `agentcore::tool`：`AgentTool` trait（name / description / schema / execute）
- 工具实现放 `agent/tools/`，每个模块一个 `pub fn tools(ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>>` 工厂
- 大部分工具用 `#[hai_macros::tool]` 宏自动生成 `impl AgentTool`
- args struct 用 `#[derive(Deserialize, JsonSchema)]` + doc comments → `schemars` 自动生成 JSON Schema

## MCP

- 基于 `rmcp`（官方 MCP Rust SDK）
- `McpManager` 启动时加载所有 MCP server 配置，每个 server 对应一个 `McpServerHandle`
- 子进程 stderr 被 pipe 到 `tracing::debug!(target: "hai::mcp")`，由 `[logging] level` 控制显隐

## Embedding

- `agentcore::embedding::EmbeddingService` trait
- 所有 `Memory` 类型都嵌入（无条件）
- 向量存储在 PostgreSQL `embedding vector(N)` 列，由 `pgvector` 管理
- 搜索走 `util::pgvector`（`<->` 余弦距离 + IVFFlat 索引）
- 维度从 `[multimodal.embedding.dimension]` 读取

## 错误处理

- `?` 优先于 `let _ =`。调用方应感知错误并决定如何处理（propagate、fallback 或 log）。
- `let _ =` 仅用于确认错误无关紧要的场景（如 best-effort cleanup、`send_typing`）。
- `if let Err(e) = ... { tracing::warn!(...) }` 替代 `let _ =`，确保错误有上下文。

## 编码风格

- `imports_granularity = "Crate"`, `group_imports = "StdExternalCrate"`
- nightly toolchain, edition 2024
- 无 CI / 无 pre-commit
- `pub(super)` 对 `runtime/` 内可见；`pub(crate)` 对 `agent/` 内可见；TUI 模块内部一律零 visibility 标记
- 倾向 RAII 封装（`HeartbeatTask`、`ContainerGuard`）和语义封装
- Service 直接调 toasty ORM，无 repo 层

## Bot 配置

```toml
[bot.telegram]
bot-token = "xxx"
allowed-chat-ids = [123456]
```

Config 覆盖链：`.hai/config.toml` → `HAI_` 环境变量 → 运行时热加载。
`HAI_LOCAL_MODE=1` 强制使用 `.hai/`，否则回退 `$XDG_CONFIG_HOME/hai/`。

## Provider

- `api_key` 是 `Option<String>`（Ollama 可省略）
- 已知 backend 需显式注册 `[providers.*]`
