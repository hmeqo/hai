# architecture

> Layering, the channel traits, the event system, the module map. Read before touching cross-module wiring, dependency direction or event persistence.

## Architecture declaration

Layered top-down — `bootstrap → platform → agent → agentcore + config + domain`, over `util` / `error` — with dependencies pointing downward. `agentcore` is a core module with no `agent`/`domain`/`config` dependencies, `config` is configuration data plus loading, `agent/model` resolves model roles, the model catalog and provider availability (including the startup reachability probe), `domain` is the data layer, and `config/conversation` holds HAI's shared Conversation values (`Language`, `ContentPolicy`, `ConversationScope`). No platform detail reaches the agent layer except through the channel traits (`agent/channel.rs:Channel` outbound, `ChannelAccess` inbound). No cycle remains: every edge runs downward. Adding an upward dependency is a defect.

### Measured dependencies

```txt
bootstrap → platform + agent + config + error
platform → agent → agentcore + domain + config
                     ↘  domain → agentcore (the embedding and rerank traits) + config
config → agentcore (ModelKind, ProviderKind, mcp::McpConfig)
domain → config (shared Conversation values and database bootstrap)
agentcore → util (text) + error
cli → bootstrap + agent + agentcore + config + domain + error
error.rs → nothing internal
```

- **`agentcore` names no config type**: the MCP block's own types (`agentcore/mcp.rs:McpConfig`, `McpTransport`) live with the client that reads them, and `agentcore/mcp.rs:McpManager::from_config` takes the `[mcp]` map instead of the whole `AppConfig`, so every cross-edge above is downward.

Nothing else points upward: the process container lives in `agent/process/context.rs:AppContext` — the lowest layer that can name every one of its parts (config, domain services, agent settings and the event bus) — so only the composition root enters it (`bootstrap.rs:serve` for the bot, `cli/mod.rs:Cli::execute` for a management command).

`agentcore` referencing `crate::agent`, `crate::domain` or `crate::config` is the one hard prohibition; `crates/hai/tests/layering.rs` asserts it, along with the rest of the table above.

### The channel traits

Two agent-owned traits, the handle the agent acts through, the platform layer's own adapter contract, and two shared pieces:

- `agent/channel.rs:Channel` — what the platform layer implements for one configured channel: its key, its profile, its declaration, and the verbs that act on a chat (send something, start the typing indicator, read a platform file). The agent acts through `agent/channel.rs:ChannelHandle`, which binds one chat to that channel, so no caller re-pairs a chat id with a channel; the outbound vocabulary is `ReplyTo` (where in the chat: topic, reference, quote — where `reference` is an agent-side message id and `quote` narrows it to a part, never a platform value) and `OutboundMessage` (that plus the chat and the payload). A send's receipt (`platform/adapter.rs:SentMessage`/`SentAttachment`) and the adapter's own message types belong to the platform layer, because that layer is their only reader. What the renderer produces is the renderer's own type (`agent/prompt/context/types.rs:BuiltContext`).
- `agent/channel.rs:ChannelAccess` — what the inbound tail is given: whether a chat may talk to this channel, where a private message goes, host copy, and a host file. A chat crosses as `domain/vo/chat_address.rs:ChatAddress`: its optional remote origin plus the platform id. `agent/channel.rs:ChannelChoices` is the separate page surface — `ask`, its in-place rewrite, the answer to a press and the page's removal — implemented only by a platform that can show one. `platform/adapter.rs:ChannelAdapter` declares `ChannelAccess` as a supertrait, so the adapter the tail receives provably satisfies the core, and no forwarding code exists.
- `platform/adapter.rs:ChannelAdapter` — the platform layer's own contract, implemented once per platform and named by no agent code.
- `platform/outbound.rs:OutboundRecorder` — implements `Channel` over a `ChannelAdapter`: it resolves an internal target to the platform's address, holds the reference to the conversational rules, and records the row that says the message left. An adapter reads no domain rows and holds no process container.
- `agent/inbound/mod.rs:Intake` — the inbound tail, agent-owned because everything it reaches for is agent or domain vocabulary; a platform only reads its own updates into `agent/inbound/arrival.rs:Arrival` and hands it over.
- `platform/host.rs:{ChannelContext, ChannelRuntime}` — one channel as the host holds it: the adapter, the one inbound path, the agents living in its chats, and the token their shutdown hangs on; `ChannelRuntime::assemble` builds all of it from a platform's adapter, so that part is written once.

**The declaration carries only what a reader varies with.** The send tool derives its advertised schema and its runtime validation from `ChannelCapabilities`, and that is what it holds. A field with no reader is not added — a platform's ability to deliver edits, say, is exercised by that platform's arrival mapping, not declared, and whether it shows a typing indicator is not declared either: the platform answers `Channel::start_typing` with the guard or with nothing (`agent/channel.rs:TypingGuard`).

**Platform autonomy**: the agent boundary carries neutral semantics (`ChatType`, `EntryAudience`, the message text and its attachments), and the store keeps that neutral shape — a platform adapter reads its own update into it on the way in (`Arrival`, whose own fields are platform-free: extras ride in `meta`) and reports the same shape on the way out (`SentMessage`). `domain` may carry platform *variants*, but no platform crate types.

## Key data flows

### Platform event to persisted events

```mermaid
flowchart LR
    D[inbound tail<br/>row + entry, one transaction] -->|1| DB[(PostgreSQL)]
    D -->|2 doorbell| I[agent Doorbell]
    I -->|3 re-read the accumulation| S[scheduler: timing decision]
    S -->|4 Ready| P[dispatch: take the batch, build prompt]
    P -->|5 spawn turn| L[Step loop: LLM + tools]
    L -->|6 consume the batch| DB
    L -->|7 events| BUS[event bus]
    BUS -->|8 batched| DB
```

The sequence carries the design: **a change is durable before anything is rung** (the row and its accumulation entry land together, so the agent always reads one state); **the scheduler only times** (it owns no side effects and never reads the database); **a Turn is the atomic unit** (the batch it took is consumed only if it ends normally, so a failure needs no repair); and **events are batched out of band**, so auditing never blocks a reply.

### The command boundary

A message one of our own command lines names never reaches ingest: the inbound tail hands it to `agent/command/handle.rs:execute` with a `CommandContext`, and a name outside the table falls back to ordinary conversation. A command never becomes a message row. What one may leave in the accumulation instead is one entry, and that is the whole of its effect on the conversation:

- an agent task appends an `EntryKind::Command` carrying the `AgentCommand`, so it survives a crash and is answered in the conversation;
- a setting change (`/persona`, `/model`, `/lang`, `/content-policy`) appends an `EntryKind::HostAction` with audience `Context` — durable before the host speaks, answering no wake reason, moving no scheduler clock, read by the next Turn as `<host_actions>` and consumed with that Turn.

Nothing above the command layer waits on the agent: the platform answers the person straight away, and the `HostAction` is ground for the next Turn rather than an instruction. `docs/topics/commands.md` owns the reply shapes and the fallback a channel without pages uses.

### Event system

- `domain/vo/event.rs:AgentEventPayload` is the typed payload enum (serde tag `event`, kebab-case names); `AgentEvent` adds the chat id and — for a model run — its `SessionRef`, and serializes into the table.
- `agent/events/bus.rs:AgentEventBus` buffers emissions and flushes them in batches into the `event` table under the domain `agent`. The table has no chat, kind or session column: all three live inside the JSONB payload, which is what the query indexes are built on (`idx_event_payload_session` serves the prefix scan).
- Two Event semantics matter beyond their names. `ModelRetry{ResponseWithText}` is **not** a retry — it records a speaking-contract violation and the loop continues; only network-class failures, throttling and gateway statuses, and an unparsable response body are actually retried. And the wrap-up events bracket the retention pass, with `WrapUpCompleted` carrying the wrapped chapter's Step count, the pass's own context tokens and its summary; the pass's own session is where those records land.
- `cli/display.rs:tag_for_kind` names the display tag of a payload — an exhaustive `match`, so a new variant fails to compile; the TUI and the `--kind` filter read the same `EventKind::as_str()`, so there is no second hand-written map to keep in step.

### Two HTTP clients

| Path | Client | Why |
|---|---|---|
| Main conversation | genai (`agentcore/provider.rs:create_genai_client`) | multi-provider adaptation, including the adapters for OpenAI-compatible vendors |
| Embedding, speech, multimodal, image generation, rerank | `agentcore::ApiClient` | a small REST client for the tooling endpoints, under the project's own control |

### Errors

One `AppError` enum, no codes or families, boundaries consuming the display string — the shape and its conventions are in `docs/topics/config.md`.

## Module map

| Module | Responsibility |
|---|---|
| bootstrap.rs | composition root: logging, bot startup and shutdown orchestration |
| platform/ | channel lifecycle host, the process-wide `ChannelRouter` and `AgentRegistry`, what each channel keeps (`platform/host.rs:ChannelContext`), the outbound recorder, and each platform's adapter. Telegram: `dispatcher` + `inbound/` — where an update becomes an `Arrival` and where a slash line's owner is decided — `outbound/` (send service, markdown, reply resolution, receipts, typing), `wire/` (the Bot API calls), `ui/` (menu and send forms), plus `builder` and `config`. TeamSpeak: `builder` (assembly and backoff), `connection` (one `SyncConnection` event task, connect, join wait), `inbound` (message→`Arrival`, chat ids), `adapter` (text send, chunking, digest upload/private text, capability answers), and `config` (section keys and identity path) |
| agent/ | the assistant's own layers, one directory per responsibility: `inbound/` (the arrival and the tail that routes it), `command/` (the slash-command table, its grammar and menu, the dialog engine, the pages `/purge` and `/digest` draw with the passes they start, and the `HostAction` a setting change records), `chat/` (one Conversation's agent: state, scheduling, origin-Chat dispatch, its shared conversation), `run/` (one run of the model: the engine, the Step loop and its record), `events/` (audit bus, doorbell, wake vocabulary), `model/` (model-role and model-catalog resolution, provider availability), `prompt/` (the role specs, the context rendering and the layer text the model reads), `environment/` (sandbox and workspace), `process/` (the container `agent/process/context.rs:AppContext`, the handles, the registry and the process timers), with `channel.rs`, `settings.rs` and `host_copy.rs` at its root, plus tools, multimodal, the analysis pipeline and networking |
| agentcore/ | core module: tool abstraction, MCP, provider clients, the model-kind table (which protocol each kind speaks), skills, XML rendering |
| config/conversation/ | shared Conversation values: `Language`, `ContentPolicy`, `ConversationScope`; imported directly where they are needed |
| domain/ | data layer: models, repos (including the runtime embedding columns), services (the entity facades and the retrieval pipeline), value objects, the chunker, the schema, and `domain/storage.rs` for database bootstrap, schema application, embedding-schema migration and `CREATE DATABASE` |
| config/ | the configuration schema, `config/load/` startup loading, and the `[model-roles]`/`[models]`/`[providers]` data the agent resolves |
| cli/ | command surface and the event-log TUI |
| util/ error.rs | text helpers and the file cache, the single error type |

## Evolution Direction

- **Startup scan**: reopen chapters that are overdue while idle, with throttling (the interface is left open today).
- **New platform**: a `platform/<name>/` submodule whose adapter implements `platform/adapter.rs:ChannelAdapter` — which includes the inbound core (`agent/channel.rs:ChannelAccess`: admission, the private-chat address, host copy and a host file), so no glue is needed; a platform that can show a page also implements `agent/channel.rs:ChannelChoices` and answers `platform/adapter.rs:ChannelAdapter::choice_surface` with it, while one that cannot inherits `None` and never allocates a page table — its declaration, the update stream read into `agent/inbound/arrival.rs:Arrival`, the send calls answering with a `platform/adapter.rs:SentMessage`, deleting messages on the platform, host copy, and file access — plus its own config struct parsed from its `[channel.<name>]` section; one arm in `platform/host.rs:ChannelHost::spawn_all`; and one variant in `domain/model/account.rs:Platform` (plus `PlatformAccountMeta` when accounts carry platform-specific display data). Beyond that, `agent/` is unchanged: admission, the command fork, ingest and the doorbell are the shared tail's (`agent/inbound/mod.rs:Intake`), and the sent row is the shared recorder's. A sender id that is not already its private chat id is the one neutral answer the adapter must supply (`ChannelAccess::private_chat_external_id`).
- **Crate split**: not now — one process and no external consumer of the library surface. Revisit when compile time degrades or a second process needs `domain`/`agentcore` at crate granularity.
- **Deliberately deferred, not to be fixed opportunistically**: the event tag spellings living in three places; a row and its vector still written non-atomically on the perception path (it is a cache and that write is best-effort by design, `docs/topics/domain.md`); and the hand-written JSON keys in the conversation snapshot.
