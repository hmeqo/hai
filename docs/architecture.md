# hai 架构

```mermaid
graph TB
  TG["Telegram API"]

  subgraph Platform["Platform Layer"]
    BH["BotHandler<br/>消息接收/命令路由"]
    SH["SignalHandler<br/>消息发送/持久化"]
  end

  subgraph Core["Core"]
    EH["EventHandler<br/>事件路由 + chat session"]
    CF["ContextFactory<br/>数据加载 + 向量检索"]
    AGENT["MainAgent<br/>ReActAgent + SystemPrompt"]
    TOOLS["Tools"]
  end

  subgraph App["AppContext (DI)"]
    CFG["Config"]
    MULTI["Multimodal"]
    DB["DbServices"]
    AGT_SVC["Agent Services<br/>Personality / GroupTrigger / Attachment"]
  end

  PG[("PostgreSQL")]

  %% 主事件流
  TG --> BH
  BH --> EH
  EH --> CF
  CF --> AGENT
  AGENT --> TOOLS
  AGENT --> SH
  SH --> TG

  %% AppContext 对外接线
  App -.-> BH
  App -.-> SH
  App -.-> EH
  App -.-> CF
  App -.-> TOOLS

  %% 数据依赖
  CF --> DB
  CF --> MULTI
  TOOLS --> DB
  TOOLS --> MULTI
  DB --> PG

  classDef infra fill:#f5f5f5
  classDef core fill:#e3f2fd
  classDef app fill:#fff3e0
  classDef store fill:#fce4ec

  class BH,SH infra
  class EH,CF,AGENT,TOOLS core
  class CFG,MULTI,DB,AGT_SVC app
  class PG store
```

## 事件流

```
Telegram → BotHandler → EventHandler → ContextFactory → MainAgent → SignalHandler → Telegram
```

多 chat 并行，单 chat 串行。

## Context 渲染顺序

`<situation>` → `<environment>` → `<chat>` → `<accounts>` → `<related_memories>` → `<related_topics>` → `<current_topics>` → `<scratchpad>` → `<perceptions>` → `<conversation>`

## System Prompt 叠加

`personality_context()` → scene → `TOOL_MANUAL` → user `system_prompt` → Skills

## 层次依赖

`entity → vo → repo → service → agent → app/context.rs`，`infra` 不依赖上层。
