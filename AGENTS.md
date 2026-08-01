# HAI — 项目文档

> 本文件是唯一文档源：既包含开发命令、架构、编码风格（每次会话自动加载），
> 也包含完整设计规格（实体、通信模型、生命周期、设计决策）。
> 曾拆分为 `AGENTS.md` + `SPEC.md` 两文件，但 agent 可能跳过 SPEC.md 导致实现偏离设计意图，故合并。
> 长会话中开场上下文失焦时，硬性规则见本文件头部「硬性规则」一节。

**每次修改代码后，必须同步更新本文件，确保文档与实现一致。**

## 硬性规则

- 术语必须遵循本文档「术语对照」表（Chat/Account/Topic/Memory/Run/Turn/Compact 等），禁止自造叫法
- 分层不可破坏：`agentcore/` 不得依赖 `agent/` 和 `domain/`
- 已废弃设计禁止回归：scratchpad、ephemeral mode、tiktoken token counting
- WakeEvent 是纯通知，不携带消息内容或 DB message_id
- 改动 session/通信/记忆/配置相关代码前，先重读本文档对应章节
- 每次修改代码后，必须同步更新本文档，确保文档与实现一致

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

- ORM: `toasty`（tokio 团队，0.8），模型定义在 `domain/model/`
- schema 由 `toasty-cli` 管理：`migration generate` → 审查 SQL → `migration apply`
- JSONB 列用 `toasty::Json<T>` 包装
- `jiff::Timestamp` 原生支持（toasty jiff feature）

## 架构

```txt
hai/src/
├── agent/               agent 业务逻辑
│   ├── node/               agent 节点定义
│   │   └── main/           MainAgent（SystemPromptBuilder + ReactLoopConfig）
│   ├── runtime/            共享运行时
│   │   ├── context.rs       RunContext + ToolContext（handler: Arc<dyn PlatformHandler>）
│   │   ├── types.rs         Messages / RunOutput
│   │   ├── react.rs         ReactRun + run_react_loop（纯函数，node 无关）
│   │   ├── run.rs           AgentRuntime（build_prompt + spawn_run + spawn_compact 执行引擎）
│   │   ├── engine.rs        AgentEngine（Arc 共享单例）
│   │   ├── registry.rs      SessionManager（Sessions HashMap + lazy retain）
│   │   ├── shell.rs         ShellRuntime（沙箱 shell，RAII ContainerGuard）
│   │   ├── event/           WakeEvent + AgentEvent + AgentEventBus
│   │   │   ├── wake.rs      WakeEvent + WakeReason
│   │   │   └── bus.rs       AgentEventPayload（serde tagged enum）+ AgentEventBus
│   │   └── session/         AgentSession（事件循环 + 调度 + 持久化）
│   │       ├── mod.rs        AgentSession 结构体 + SessionState 枚举（Idle / Busy，含 runtime + run_count）
│   │       ├── event_loop.rs  run() 主循环 + idle_tick + Busy handler
│   │       ├── dispatch.rs    dispatch + on_complete + build_run_context + gather_messages
│   │       ├── proxy.rs       SessionHandle + SessionStatus + HeartbeatTask
│   │       ├── conversation.rs Conversation（纯状态容器：messages + turns + since_id + 去重）
│   │       ├── scheduler.rs   EventScheduler（pure timing + AttentionConfig）
│   │       └── attention.rs   Heat + Window（调度器数学原语）
│   ├── tools/              工具实现（sleep/think/done/message/voice/topic/memory/shell 等）
│   ├── context/            提示词渲染（XML → prompt string）
│   ├── link.rs             BotId + BotProfile + PlatformHandler trait + MessageCapability
│   ├── personality/        性格配置渲染
│   └── multimodal/         多媒体服务
├── agentcore/           agent 核心库（不依赖 agent/domain）
│   ├── tool.rs          AgentTool trait + ToolError + 辅助函数
│   ├── mcp.rs           McpManager + McpServerHandle（基于 rmcp）
│   ├── embedding.rs     EmbeddingService trait
│   ├── provider.rs      ProviderBackend + genai Client 工厂
│   ├── skills/          SkillManager
│   ├── render/          渲染引擎（XML/JSON/MD）
│   └── mod.rs
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
│   └── vo/              值对象（ChatId, MessageId, Turn, ToolCallResult, ConversationSnapshot, TopicSearchResult 等）
├── platform/            Telegram 平台集成
│   └── telegram/
│       ├── builder.rs         TelegramPlatform::spawn() 自举 + start/stop 日志
│       ├── dispatcher.rs      teloxide 路由（薄层）
│       └── message_handler.rs  消息处理（账号解析 + 持久化 + 事件分发）
├── config/              配置系统
└── app/                 应用上下文 + 启动
```

核心分层原则：

- `agentcore/` 完全不依赖 `agent/` 和 `domain/`
- `agent/` 依赖 `agentcore/` 和 `domain/`
- `domain/` 定义模型和服务，不依赖 agent 逻辑

## 编码风格

- `imports_granularity = "Crate"`, `group_imports = "StdExternalCrate"`
- nightly toolchain, edition 2024
- 无 CI / 无 pre-commit
- 作用域限制优先在 mod 级别用 `pub(xxx) mod`，其次 `pub(xxx) struct`，少用 `pub(xxx) fn`——过多的 `pub(xxx)` 是噪声。`super` 还是 `crate` 按需选择
- `let _handle =` 是 `JoinHandle` 的标准 discard 写法（`_` 前缀压制 unused）
- 倾向 RAII 封装（`HeartbeatTask`、`ContainerGuard`）
- Service 直接调 toasty ORM，无 repo 层

## 项目定位

HAI 是一个个人 AI 助手，以 Telegram Bot 形态运行。它不是客服机器人或通用对话 AI，而是一个有自己个性、会主动关注你、可以记住你偏好和话题的长期陪伴型 agent。

### 核心场景

- **私聊陪伴**：一对一聊天，agent 记住你的个人事实（UserFact）、创建话题（Topic）管理长对话、在话题结束时自动归档
- **群聊参与**：agent 在群里选择性回复，不是每条消息都接。通过 @提及、回复触发高响应模式，否则以概率观察模式参与
- **媒体理解**：图片描述、语音转文字、视频理解、音频分析（OCR），通过 multimodal pipeline 输出结构化感知结果
- **工具执行**：agent 可调用工具来搜索记忆/话题、执行 Shell 命令（沙箱容器）、发送消息/语音、操作 scratchpad 等
- **长期记忆**：通过 Memory 系统（UserFact / Note / Knowledge）持久化重要信息，自动嵌入向量用于语义检索
- **MCP 扩展**：通过 Model Context Protocol 接入外部 MCP server，动态扩展 agent 能力

## 核心概念

### 实体

**Chat（聊天）**：
一个 Telegram 会话，类型可以是 Private（私聊）、Group（群组）、Supergroup（超级群组）、Channel（频道）。
每个 Chat 有一个 AgentSession 管理其事件循环。Chat 通过 `chat_id: i64` 标识，存储在 `chat` 表。

**Account（平台账号）**：
一个用户在某个平台上的身份。目前只有 Telegram 平台。
每个 Account 携带平台特定的元数据（如 first_name、username），存储在 `account` 表。
Account 可关联到 Identity 实现跨平台身份绑定（未来）。

**Identity（身份）**：
跨平台的用户身份。未来用于绑定多个平台的 Account（如 Telegram + QQ）使记忆共享。
目前 Identity 是可选关联，account.identity_id 可为空。

**Topic（话题）**：
Chat 内的一组相关消息构成一个话题。话题有 Active 和 Closed 两种状态。
Closed 的话题不可再追加消息，agent 可以用工具重新打开。
话题支持父子层级（parent_topic_id），用于话题细分。
话题闲置超过 `topic_idle_hours` 小时后会被自动标记为 need-close。
话题的 title/summary 由 agent 通过工具维护。话题内容会做嵌入向量用于语义检索。

**Message（消息）**：
Chat 中的一条消息，由 role（user/assistant/system）、content（JSONB 结构化的内容片段列表）、interaction_status（Unread/Replied/Seen）等字段构成。
消息内容可以是多段的 TelegramContentPart（text/photo/video/audio/voice/document/sticker/animation/video_note）。
每条 agent 发出的消息会记录 external_id（平台侧消息 ID）和 sent_at。

**Memory（记忆）**：
持久化的知识条目，有三种类型：

- **UserFact**：关于用户个人的事实（"他养了一只猫叫咪咪"），关联 account_id
- **Note**：agent 自己写的笔记（"用户喜欢在深夜聊哲学"），关联 chat_id
- **Knowledge**：通用知识条目（"Rust 的 borrow checker 规则是……"），无特定归属
所有 Memory 无条件进行向量嵌入，搜索时三种类型一起召回，不做类型排除。
Memory 有 importance 字段（1-5），但当前未参与排序逻辑，仅作预留。

**Perception（感知）**：
多模态分析的结果缓存。每条记录包含 source（文件来源标识）、parser（分析器类型：Image/Audio/Video/Ocr）、prompt（分析提示词）和 content（分析结果文本）。
用于避免重复分析同一文件。

**Event（事件）**：
Agent 运行的可观测事件，存储在 `event` 表，payload 是 JSONB。
事件类型由 `AgentEventPayload` 枚举定义，通过 `#[serde(tag = "event")]` 序列化。

### 事件类型（AgentEventPayload）

所有事件统一存在 `event` 表的 `payload` JSONB 列，用 `chat_id` 字段区分归属。

| 事件               | tag                 | 触发时机                  | 关键字段                                                                     |
| ------------------ | ------------------- | ------------------------- | ---------------------------------------------------------------------------- |
| `RunStarted`       | `run_started`       | 开始一次 processing       | run, reason, msg_count, full_prompt                                          |
| `ToolCall`         | `tool_call`         | 工具被调用                | run, tool, args                                                              |
| `ToolCallResult`   | `tool_call_result`  | 工具返回结果              | run, tool, summary, success                                                  |
| `TurnCompleted`    | `turn_completed`    | 一次 LLM 调用完成         | run, turn, reasoning, response                                               |
| `RunCompleted`     | `run_completed`     | 一次 run 成功完成         | output, tool_calls, elapsed_ms, prompt_tokens, completion_tokens, has_spoken |
| `ModelRetry`       | `model_retry`       | 模型需要重试              | run, reason (ResponseWithText/TimeoutRetry)                                  |
| `Preempted`        | `preempted`         | run 期间 inbox 新事件注入 | run, count, reasons, content                                                 |
| `RunFailed`        | `run_failed`        | run 失败                  | run, elapsed_ms, error                                                       |
| `CompactCompleted` | `compact_completed` | 章节压缩完成              | run_count                                                                    |

### 术语对照

| 术语       | 含义                                                  | 避免混淆                     |
| ---------- | ----------------------------------------------------- | ---------------------------- |
| Chat       | Telegram 会话（Private/Group/Supergroup/Channel）     | 不要叫"房间"或"对话"         |
| Account    | 平台用户账号（一个平台一个）                          | 不要叫"用户"或"身份"         |
| Identity   | 跨平台用户身份（预留）                                | 不要和 Account 混用          |
| Topic      | Chat 内的话题，可 Active/Closed                       | 不要叫"主题"或"线程"         |
| Memory     | 持久化知识条目（UserFact/Note/Knowledge）             | 不要叫"记忆片段"或"知识库"   |
| Run        | 一次 react-loop 执行                                  | 不要叫"轮次"                 |
| Turn       | 一次 LLM exec_chat 调用（含工具调用）                 | 不要叫"步骤"                 |
| Compact    | LLM 压缩的章节摘要                                    | 不要叫"总结"或"概要"         |
| Session    | 每个 Chat 一个 AgentSession（Idle/Active/Compacting） | 不要叫"连接"或"实例"         |
| WakeEvent  | 外部触发的 session 唤醒信号                           | 不要叫"通知"或"请求"         |
| Inbox      | Session 的事件输入队列                                | 不要叫"缓冲区"               |
| Handler    | PlatformHandler trait 实现                            | 不要叫"驱动"或"适配器"       |
| Perception | 多模态分析的结果缓存                                  | 不要叫"分析记录"或"感知数据" |
| Heat       | 概率注意力机制的基础值                                | 不要叫"热度"或"权重"         |
| Window     | 注意力时间窗口                                        | 不要叫"窗口期"               |

## 通信模型

### 外部到 Session

```txt
Platform → SessionHandle.wake(WakeEvent)
  → Inbox.push(event) + Notify
    → idle_tick (select! 从 notified/status/deadline 中醒来)
      → inbox.drain → scheduler.enqueue → scheduler.decide
        → Ready(events) → dispatch → spawn_run
        → Defer → idle loop 继续
        → Done (窗口关闭 + 超时) → compact → 退出
```

### 内部通信

| 从         | 到       | 方式                                       | 数据                 |
| ---------- | -------- | ------------------------------------------ | -------------------- |
| Platform   | Session  | SessionHandle.wake() → Inbox.push + Notify | WakeEvent            |
| Idle       | Active   | idle_tick → dispatch                       | context + WakeEvents |
| 正在处理   | 完成     | oneshot::Receiver                          | RunOutput            |
| 状态查询   | Session  | mpsc + oneshot                             | SessionStatus        |
| 事件持久化 | EventBus | 异步写入                                   | AgentEventPayload    |

### Session 状态机

```txt
Idle ──(dispatch)──→ Busy{handle, result_rx, started_at}
Busy ──(on_complete)──→ Idle
Busy ──(on_compact_done)──→ Idle（或退出，看是否有待处理事件）
Busy ──(Failed/Cancelled)──→ Idle（或退出）
Idle ──(window close + idle timeout + run_count > 0)──→ Compact → Busy
Idle ──(window close + idle timeout + run_count = 0)──→ Exit
```

### 事件调度（EventScheduler）

调度器是纯 timing engine，持有唯一挂起事件队列。

**调度策略**：

| 方法             | 触发条件                               | 效果                                               |
| ---------------- | -------------------------------------- | -------------------------------------------------- |
| `is_addressed()` | Direct / Mention                       | 刷新窗口（reset window timer）、重置热量           |
| `is_rapid()`     | Direct / Mention / Scheduled / Command | 跳过 debounce（立即调度）                          |
| `is_mergeable()` | Observe / Mention / Direct             | 同类事件合并处理                                   |
| debounce         | 仅 Observe                             | 最后一次事件后等 1500ms 才 dispatch                |
| Heat spend       | random < heat.value                    | 概率性 dispatch Observe 事件（模拟真人随机看消息） |
| Window           | 被 @/回复后触发                        | 期间内事件全部 dispatch（活跃期）                  |

**调度决策**：

```txt
scheduler.decide():
  if 在 debounce 期内 → Defer
  if 窗口激活:
    if 队列空 → Defer
    else → Ready（取所有事件）
  if random < heat → Ready（花掉 heat）
  if 窗口关闭 + 超时 → Done（或 Ready 如果还有事件）
  else → Defer
```

### 核心设计概念

**WakeEvent（通知）**：
WakeEvent 是一个纯通知，**不携带消息内容或 DB message_id**。它的作用只是告诉 session"有新情况"。
具体原因：就像手机消息通知，你看到通知打开软件，自然从上次看到的地方读起。
WakeReason 携带的是通知类型（Direct/Mention/Observe/Scheduled/Command），用于调度器做 debounce/window/heat 决策。

```
平台收到消息 → 持久化到 DB (interaction_status=unread)
  → WakeEvent { reason } push 到 Inbox
    → session 被唤醒 → gather_messages(since_id) → 从 DB 拉消息
```

**Preempt**：
Run 正在进行时，Inbox 收到了新的 WakeEvent。当前 turn 结束后，react loop 会 drain inbox 并调用 `build_situation_section` 将事件信息渲染为 XML 注入 LLM 的下一轮上下文。
Preempt 不 fetch 消息全文，不传 message_id。agent 看到"有新消息来了"的通知后自行判断如何行动（等一会、结束当前 run 让下个 run 处理、继续当前策略等）。
新消息的内容由下次 dispatch 的 `gather_messages(since_id)` 自然拉到。

```
[turn N]
  工具执行结束
  inject_preempt:
    inbox.drain() → WakeEvents → coalesce → <situation><trigger .../></situation>
    push 到下一轮 LLM 上下文
  [turn N+1] agent 看到 situation 通知，决定下一步
```

**`since_id`（游标）**：
记录会话已读到哪条 DB 消息。每次 `gather_messages` 用 `since_id` 作为 fetch 起点（`SELECT WHERE id > since_id`）。
Run 结束时 `since_id` 更新为本次 fetch 到的最后一条消息 ID。下个 run 从新的 `since_id` 开始。

**`message_ids`（本次覆盖的消息 ID 列表）**：
`dispatch` 时 `gather_messages` fetch 到的所有消息 ID。用于 run 结束后 `mark_seen`。
同时 run 结束后会补查一次 `get_messages_window(since_id)`，把 run 期间 preempt 覆盖的消息也纳入，然后一起调 `mark_seen`。
这样 preempt 消息也能被正确标记已读，不需要 WakeEvent 携带 message_id。

```
dispatch 时: message_ids = gather_messages 取到的 ID 列表
run 期间:   preempt 消耗 inbox 事件（不传 ID）
结束后:     补查 get_messages_window(since_id) → extra_ids
            mark_seen(message_ids ++ extra_ids)
```

**`mark_seen`**：
由 spawned task（react loop）在 run 成功后调 DB 标记消息已读。
标记范围 = dispatch 时 fetch 的 + run 期间通过 preempt 覆盖的。
不做全量标记、不做猜测，只标记本次 run 确凿覆盖的消息。

## Session 详细生命周期

### Session 创建

`AgentSession::new()` 在首次收到 chat 的 WakeEvent 时创建（由 SessionManager 管理，延迟初始化）。

创建参数：

- `engine`: AgentEngine（Arc 共享）
- `chat_id` / `chat_type`: 从 DB 加载
- `handler`: PlatformHandler 实现
- `shell`: 沙箱 shell（Arc<Mutex>）
- `base_heat` / `window_secs`: 从 PersonalityConfig 读取
- `conversation`: 从 ConversationRecord 恢复，无记录则新建

### 一次 Run 的执行流程

```txt
dispatch(events):
  build_run_context(events) → RunContext
  gather_messages() → DB messages
  set_since_id(next_since_id)
  runtime.build_prompt(ctx, messages, shown_ids, is_first) → BuiltContext
  record_shown + build_full_messages → RunInput
  emit RunStarted event
  runtime.spawn_run(ctx, payload, inbox, run_number) → (handle, result_rx)
  切换到 Busy{handle, result_rx, started_at} 状态

react_loop:
  loop:
    build_request(messages + system prompt + tools)
    exec_chat → response
    if response has tool calls:
      execute tools in parallel
      emit ToolCall / ToolCallResult events
      add results to messages
      continue
    else:
      emit TurnCompleted
      if 下次请求有新事件 → preempt, continue
      break → return ReactLoopOutput

on_complete(output):
  has_spoken = check output.turns
  if has_spoken: schedule.refresh()
  conversation.update(output.turns, output.messages)  // 更新 context_tokens 为最后一个 turn 的 prompt_tokens
  run_count += 1
  snapshot → persist ConversationSnapshot via ConversationRecordService
  drain inbox → schedule.enqueue → Idle

on_failed:
  log error
  emit RunFailed
  drain inbox → schedule.enqueue → Idle
```

### Compact 流程

发生在 session idle timeout 触发 `Decision::Done` 且 `run_count > 0` 时。

```txt
Idle → should_compact → Busy{compact_result_rx}:
  runtime.spawn_compact(messages) → (handle, result_rx)
  select! { status_query, compact_result }
  - status query：回复后继续等待
  - compact 完成：open_new_chapter(compact) → run_count=0 → save → drain inbox → Idle or exit
  - compact 失败：drain inbox → Idle or exit
```

`open_new_chapter(compact)`：

- conversations.messages = [user(compact)]
- 清空 shown_memory_ids、shown_topic_ids、last_turns
- 重置 prompt_tokens = 0、run_count = 0
- since_id 不变（继续追踪 DB messages 的增量）

## Memory 系统

### 数据结构

```rust
Memory {
    id: Uuid,                    // PK, UUID v7
    chat_id: Option<i64>,        // Note 使用
    account_id: Option<i64>,     // UserFact 使用
    kind: String,                // "user_fact" | "note" | "knowledge"
    content: String,             // 内容文本
    importance: i32,             // 1-5，预留排序，当前未使用
    meta: Option<Json<Value>>,   // 通用元数据（references 等）
    created_at: Timestamp,
    updated_at: Timestamp,
}
```

### 类型

| 类型      | tag       | chat_id | account_id | 用途                         |
| --------- | --------- | ------- | ---------- | ---------------------------- |
| UserFact  | user_fact |         | ✓          | 用户个人事实（偏好、习惯等） |
| Note      | note      | ✓       |            | agent 对 chat 的笔记         |
| Knowledge | knowledge |         |            | 通用知识                     |

### 嵌入策略

- 所有 Memory 统一嵌入（无条件）
- 向量存储在 PostgreSQL `embedding vector(N)` 列，由 pgvector 管理
- 搜索走 `<->` 余弦距离 + IVFFlat 索引
- 维度从 `[multimodal.embedding.dimension]` 读取
- `search_related` 不做 type 排除，三种类型一起召回

## Tools 层

### 架构

- **定义**在 `agentcore::tool`：`AgentTool` trait（name / description / schema / execute）
- **实现**在 `agent/tools/`：每个文件一个工具集，暴露 `pub fn tools(ctx) -> Vec<Arc<dyn AgentTool>>`
- **宏**：`#[hai_macros::tool]` 自动生成 `impl AgentTool`
- **Schema**：args struct 用 `#[derive(Deserialize, JsonSchema)]` + doc comments → `schemars` 自动生成 JSON Schema

### 当前工具

- **done**：完成当前 turn（正常工具，不 pre-filter，发射 ToolCall/ToolCallResult 事件，计入 tool stats）
- **think**：记录思考过程（thought: String，no-op）
- **sleep**：等一会儿（secs: f64），感觉对方还在打字时用，让更多消息到了再一起处理
- **send_message** / **send_voice**：发送消息/语音到平台
- **topic_search** / **topic_summarize** / **topic_create** / **topic_close** / **topic_reopen**：话题管理
- **memory_search** / **memory_create** / **memory_update** / **memory_delete**：记忆管理
- **scratchpad_read** / **scratchpad_write**：暂存区操作（禁用中，model+service 保留）
- **bash**：沙箱 Shell 执行
- **MCP 工具**：动态加载，通过 McpManager.list_all_tools()

## MCP 集成

- 基于 `rmcp`（官方 MCP Rust SDK）
- `McpManager` 启动时加载所有 MCP server 配置（TOML），每个 server 对应一个 `McpServerHandle`
- 启动时 spawn 子进程，通过 stdio 通信
- 子进程 stderr 被 pipe 到 `tracing::debug!(target: "hai::mcp")`，由 `[logging] level` 控制显隐
- `list_all_tools()` 合并所有 server 的工具到 RunContext 的工具列表

## 配置系统

### 覆盖链

```txt
.hai/config.toml → HAI_ 环境变量 → 运行时热加载
```

- `HAI_LOCAL_MODE=1` 强制使用 `.hai/`，否则回退 `$XDG_CONFIG_HOME/hai/`
- 配置由 `struct_patch` 管理，支持字段级热加载更新

### Paths 系统

`Paths` 是全局惰性单例，进程启动时通过 `OnceLock` 解析一次所有路径。

```rust
Paths {
    config_dir,      // 配置目录：.hai/ 或 $XDG_CONFIG_HOME/hai/
    data_dir,        // 数据目录：.hai/ 或 $XDG_DATA_HOME/hai/
    config_file,     // config_dir + "config.toml"
    config_file_str, // 预缓存的 UTF-8 字符串
    file_cache_dir,  // data_dir + "files"
    skill_dirs,      // 存在的 skill 目录列表（config/skills, .hai/skills, .agents/skills）
}
```

### Provider 配置

```toml
[providers.openai]
type = "openai"
api-key = "sk-..."

[providers.ollama]
# type 省略时从 key 名推断
```

`ProviderConfig::infer_kind()` 解析逻辑：优先用 `type` 字段，否则从配置 block 名通过 `ProviderKind::from_str` 推断。

### Bot 配置

```toml
[bot.my-bot]
type = "telegram"          # 省略时从 key 名推断
bot-token = "xxx"
allowed-chat-ids = [123456]
```

`BotConfig::resolve()` 解析为 `BotConfig { key, platform, bot_token, allowed_chat_ids, rich_message }`。

Bot 启动流程：

1. `TelegramPlatform::spawn()` 自举身份（get_me / get_my_name / ensure_bot_account）
2. 注册 dispatcher
3. 日志记录 started / stopped

### Attention 配置

```toml
[agent.attention]
base-attention = 0.05    # 基础注意力（0-1）
window-secs = 30.0       # 注意力窗口（秒）
```

- `base_attention`：Idle 时每一轮 idle_tick 中有多少概率随机关注 chat（Observe）
- `window_secs`：被 @ 或回复后保持高响应的时间窗口
- `PersonalityTier`（Low/Mid/High）映射到不同的 base_attention 默认值

### Sandbox 配置

```toml
[sandbox]
enabled = true
runtime = "auto"       # docker / podman，留空自动检测
image = "ubuntu:latest"
timeout-secs = 30
```

- `runtime` 未指定时自动检测：优先 Podman，否则 Docker
- `ToolContext.sandbox: Option<SandboxConfig>`，启用时一组字段全有，禁用时全无

## SDK / 外部依赖

| 依赖               | 用途             | 版本策略                   |
| ------------------ | ---------------- | -------------------------- |
| genai              | LLM API 客户端   | 跟随上游                   |
| toasty             | ORM              | 0.8                        |
| teloxide           | Telegram Bot SDK | 跟随上游                   |
| rmcp               | MCP SDK          | 官方 Rust SDK              |
| pgvector           | 向量搜索         | Postgres 扩展              |
| jiff               | 时间处理         | toasty jiff feature        |
| serde / serde_json | 序列化           | 标准配置                   |
| arc_swap           | 热加载配置       | 无锁读写                   |
| struct_patch       | 配置合并         | 字段级 patch               |
| schemars           | JSON Schema 生成 | tool 参数校验              |
| strum              | 枚举推导         | IntoStaticStr + EnumString |

## 类型安全

- 所有 PK 用透明 newtype 包裹，定义在 `domain/vo/id.rs`：`ChatId(i64)`, `MessageId(i64)`, `TopicId(Uuid)`, `MemoryId(Uuid)`, `PerceptionId(Uuid)`, `IdentityId(Uuid)`, `AccountId(i64)`
- 通过 `id_type!` 宏生成 From/Into/Display/raw_ids()
- Domain model 字段用裸类型保持 ORM 兼容（`chat_id: i64` 而非 `ChatId`），VO 在 service 层转换
- JSONB 列用 `toasty::Json<T>` 包装

## 错误处理

- `?` 优先：调用方应感知错误并决定如何处理
- `let _ =` 仅用于确认错误无关紧要的场景（best-effort cleanup、send_typing）
- `if let Err(e) = ... { tracing::warn!(...) }` 替代静默吞错误，确保错误有上下文

## 关键设计决策

### 为什么不用 scratchpad

Scratchpad（暂存区）最初设计为 agent 的"便利贴"——跨 turn 保持状态。实践中 agent 误用它充当 chat 状态而非单纯的记忆 hint，导致上下文混乱。已禁用，model+service 保留供未来合理使用。

### 为什么用 compact 而非分页

Conversation 消息量不断增长时，有两种策略：

- **分页**：截断历史，保留最近 N 条
- **Compact**：LLM 总结当前对话后以 user(compact) 代替全部历史

选择 compact 的理由：

- 分页会丢失信息，compact 保留语义
- compact 在 session idle timeout 时异步执行，不阻塞正常处理
- since_id 不重置，DB messages 仍可增量查询

### 为什么 token counting 被删除

`tiktoken-rs` 算 token 的开销（计数 + 依赖）超过了收益——token 数仅在日志中展示，不参与任何调度逻辑。已删除 `agentcore/token.rs` 和 `tiktoken-rs` 依赖。

### 为什么 ephemeral mode 被移除

Ephemeral mode（不持久化 conversation）曾作为"轻量模式"存在，但实际使用中所有场景都需要持久化，模式切换增加了不必要的复杂度。统一为 continuous（持续持久化）。

### PathResolver → Paths 全局单例

原 `PathResolver` 是零大小结构体的静态方法，每次调用都重新解析 local/XDG fallback。改为 `Paths` 全局惰性单例（OnceLock），进程启动时一次解析所有路径并缓存，公开方法返回引用（&Path / &[PathBuf] / &str）。

### 事件模型为什么用 tagged enum JSONB

所有 AgentEventPayload 统一存入 `event` 表的 `payload` JSONB 列，serde tagged enum 用 `"event"` 字段区分类型。好处：

- 单表查询所有事件，无需 JOIN
- 新增事件变体无需 migration
- JSONB 可灵活存储不同类型的不同字段

### ConversationSnapshot 层边界

`Conversation`（agent 层的运行时状态）通过纯数据 VO `ConversationSnapshot` 与 domain persistence 交互。`ConversationRecordService` 只操作 Snapshot，不依赖 agent 类型。`Turn` 和 `ToolCallResult` 也归入 `domain::vo`，避免 domain 层反向依赖 agent。

### 为什么 PersonalityTier 取代 f64 字段

原系统中 personality 有 6 个 f64 字段（sociability / verbosity / honesty / humor / rationality / mood），需要复杂的 `curve()` / `dims()` 方法计算注意力参数。改为 `PersonalityTier` 枚举（Low/Mid/High），每个 tier 映射到固定的 attention 配置，删除所有计算函数，大幅简化代码。
