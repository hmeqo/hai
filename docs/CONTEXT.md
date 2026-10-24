# HAI domain language

HAI is a long-term companion AI assistant running as a Telegram bot and a TeamSpeak 3 text-chat client. This document is the authority on the project's language: what each word *is*. Mechanisms live in `docs/architecture.md` and `docs/topics/*.md`.

## Language

### Entity domain

**Conversation**: the top-level context owner — its chapters, accumulated context, pending changes, settings, memory, topics and workspace. One Conversation contains one or more Chats, possibly from different platforms.
_Avoid_: "session" (a model run), "chat" (a platform surface)

**Chat**: one platform surface inside a Conversation — a Telegram private, group or channel chat, or a TeamSpeak room, server chat or private message. It owns the transport address and alias; a Turn is always bound to exactly one origin Chat.
_Avoid_: bare "conversation" when the platform surface is meant

**origin Chat**: the member Chat a Turn is executing for. Its channel supplies the tools, sends and typing indicator; another member Chat's pending work waits for its own Turn.

**platform channel**: one configured HAI channel instance (`platform_channel`), the local trust boundary every Chat and Account hangs off. `channel_key` is the local anchor; a remote origin is metadata, never the sole identity.
_Avoid_: "chat" for the configured connection

**conversation scope**: how a channel places newly created Chats — `shared` puts every one of them in the channel's Conversation, reached through `platform_channel.default_conversation_id`, and `isolated` gives each its own. Both platforms default to `isolated`; the chat type is not part of the rule.
_Avoid_: "default conversation" when the policy is meant

**room**: a TeamSpeak channel as a text surface, stored as `channel:<id>` under the configured channel key. The code calls it a room so **channel** keeps its HAI meaning; TeamSpeak's own word for it stays "channel".
_Avoid_: "room" for a Telegram chat; "channel" when the sentence means HAI's configured instance and a TeamSpeak room at the same time; "ChatType::Channel" for a TeamSpeak room (that is a chat kind)

**conversation language**: the language a Conversation is spoken in — set per Conversation by `/lang`, otherwise the configured default. Not the user's own language, and not a fence: the agent still translates, practises a language or quotes someone when asked.
_Avoid_: "locale" (that is a platform or client concept), "user language"

**text catalog**: the text a conversation language reads — Chinese has a catalog of its own and every other language reads the English one; it spans the built-in soul and chat-type layers, the wake instruction and situation text, host copy and status labels, while the `agents` and `wrap-up` layers carry one text every catalog reads, and an operator override replaces a single layer, never a catalog.
_Avoid_: "message catalog" (a Message is a chat message), "resource bundle", "locale bundle", "built-in text set"

**client code**: the two-letter ISO 639-1 code a client platform matches a client's language by (`config/conversation/language.rs:Language::client_code`); a regional variant has no code of its own, so `en-gb` is served by `en`. The conversation language's stored value is not this — that tag is `Language::as_str`.
_Avoid_: "locale" (the client's own setting, which the bot never reads), "language code" (ambiguous with the stored tag)

**Account**: one person's identity on one configured platform channel, carrying that platform's metadata and unique by `(channel_id, external_id)`.
_Avoid_: "user" (the user is the real person)

**Topic**: one thing tracked inside one Conversation over a span of time — a title, a summary, append-only notes, and the messages it covers. Independent of chapter boundaries.
_Avoid_: "theme", "thread" (a thread is a platform concept)

**note**: one append-only piece of raw material inside a Topic; the summary is a projection over the notes, so rewriting it loses nothing.
_Avoid_: "detail", "log"

**need-close**: the hint rendered on a Topic that has been idle past `topic_idle_hours`, telling the model the topic can be closed. Rendering only — nothing is stored, and closing does not freeze the record.
_Avoid_: "closed", "archive marker"

**recall**: the model's own lookup into past memory and topics — a semantic query, lexical keywords and a time range, fused into one ranking.
_Avoid_: bare "RAG", bare "search"

**Memory**: one persisted atomic fact inside one Conversation (a user fact, a note or a knowledge entry) with a lifecycle and a pinned flag that puts it in the chapter-opening block.
_Avoid_: "knowledge base"

**Perception**: the cached result of analyzing one attachment or URL inside one Conversation — a base transcription, plus a targeted judgment when the caller names a focus. Its key is the Conversation plus `subject_kind` (`handle` = `<platform>:<file_unique_id>`, `url`), `subject`, parser and focus.

**attachment**: one occurrence of a file in one message — the occurrence id the model reads, the platform handle, the kind, the caption and the meta. Two occurrences of the same file in one Conversation are distinct rows that share one **Perception**; another Conversation has its own cache row.

**subject** / **subject_kind**: what a **Perception** row is cached against — `handle` spells `<platform>:<file_unique_id>`, falling back to the platform's download handle when no stable id was given, `url` a normalized URL, and the kind travels with the value (`domain/vo/subject.rs:Subject`). Every writer and reader spells it identically.
_Avoid_: "key" bare when the perception key is meant

**KnowledgeBase**: the global, curated document corpus, built from the CLI and read only by the agent. Distinct from Memory, which is Conversation-scoped personal memory; both exist.
_Avoid_: conflating the two

**KnowledgeDocument** / **KnowledgeChunk** / **Collection**: a document is the lifecycle unit (import, update, delete), a chunk is the retrieval unit, and a collection is the label a document carries — or none, for a document filed under nothing (`Collection::Uncategorised`).

**WebSearch**: live-web retrieval through search providers — candidate links with excerpts, neither curated nor verified. Distinct from knowledge-base retrieval (curated) and from `recall` (Conversation-scoped memory).

**web provider**: a supplier of the networking capability (parallel, exa, tavily); its identity, endpoint, tool names, credential requirement and capabilities are declared once, and both the search chain and `read`'s fetch chain use the same instances.
_Avoid_: bare "provider" when the model provider (`[providers.*]`) is meant

**provider availability**: whether a `[providers.*]` entry can be used — a credential (the kind's environment variable, or the block's `api-key`), or, for a keyless service, its endpoint answering; so a block declares an override (transport, endpoint, headers), not the provider's existence.
_Avoid_: calling `[providers.*]` "configuration" when availability is meant

**Message**: one message in a Chat — its own text plus its attachments, each an `attachment` row holding a platform file handle.
_Avoid_: "message record"

**Event**: one observable event of an agent run, its payload a tagged enum, persisted for auditing; the envelope names the chat and, for a model run, the session it belongs to.
_Avoid_: "log" (log is tracing output)

**Session**: the trace's top unit — one model run. A main session is one **Chapter**; a wrap-up and a digest each run as their own session (`SessionKind`: `main`, `wrap-up`, `digest`), and a wrap-up names the chapter it closes as its parent. A host transaction (a purge) is conversation-scoped and belongs to no session. Events carry their `SessionRef` in the envelope, so `(session, turn, step)` names a row even though turn numbers restart with every session.

**ConversationRecord**: the persisted Conversation state — accumulated context messages, the chapter scalars, and the Conversation's `pending` accumulation. Each pending entry names its origin Chat. Its row is the one place a Conversation's state lives; the execution environment is rebuilt from it.
_Avoid_: conflating with the in-memory Conversation

**Chapter**: the segmentation unit of conversation history — the current chapter plus the retention summary of the previous one. Older content is discarded.

**Chapter reopen**: opening a new chapter once no run has happened for the idle timeout, once the chapter's token count reaches `compact-token-threshold`, or after a stricter content-policy change. The wrap-up pass is one part of it, not the behavior itself; a model or persona change never reopens.
_Avoid_: "compaction", "cleanup"

### Runtime domain

**ConversationAgent**: the agent entity of one Conversation — the counterpart the user talks to. It runs the event loop with an Idle/Busy state machine — a Turn, a chapter wrap-up, a purge and a digest all hold it Busy — selects exactly one origin Chat per Turn, is created lazily per Conversation, and is rebuilt from the snapshot when needed. Its handle is `agent/chat/handle.rs:ConversationAgentHandle`.
_Avoid_: "session", "instance", "ChatAgent"

**Sandbox**: the environment the agent's tools act on — where a command runs and which paths a tool may open (`agent/environment/sandbox/mod.rs:Sandbox`; two variants, picked from `[sandbox] enabled`: a container, or the host). The workspace and its read-only mounts are the tree both sides share, so `read` and `bash` reach the same files.
_Avoid_: "shell" for the environment as a whole

**Workspace**: the one writable directory of a Conversation, given by `agent/environment/workspace.rs:Workspace::for_conversation` as `<data_dir>/workspaces/<conversation id>`, bound at `/workspace` inside a container and the default working directory there; `workspace://<path>` is how a tool addresses it, so the directory's real location never enters the model's own spelling. A generated image lands in `generated/` inside it, and nothing reaches the chat except through `send_message`.

**Turn**: the execution unit in which the agent fully responds to one wake-up — the whole loop from taking the accumulated changes to replying, possibly several Steps and tool calls. It ends as Success, Steered (a new event interrupted it, and what it already did takes effect) or Failed (an exception, with no state side effects). Numbered within its session (a main session is one chapter).
_Avoid_: "round"

**Step**: one LLM call together with that call's tool results, numbered within its Turn. Recorded at runtime, never persisted.
_Avoid_: "step" for a process description rather than an LLM call

**AgentRole**: the role the model plays in one loop — `Default`, the companion in everyday conversation, or `WrapUp`, the chapter-close record-keeper whose text output is machine-extracted. A role owns its loop: prompt, options, tool surface, stopping and steering.
_Avoid_: conflating with ModelRole; bare "role"; "persona" (a future dynamic mask above the Default role's soul)

**ModelRole**: a named use of a model — vision, transcription, speech, embedding, image or rerank — selected through the `[model-roles]` config blocks rather than an enum, each block naming a `provider/model` entry. The main model is the default.
_Avoid_: "model config"

**ModelKind**: what a ModelRole asks a provider for — `chat`, `vision`, `transcription`, `embedding`, `image`, `speech`, `rerank`. A model kind is one endpoint class of a provider, not one model, and the role names it.
_Avoid_: calling it a "surface"; "capability" as a bare synonym; bare "kind" (the provider kind is the vendor preset, the model kind is the endpoint class)

**Scorer**: the retrieval scoring stage as the domain consumes it (`agentcore/api/rerank.rs:Scorer`): one rerank transport plus the budgets every call shares. Configured through `[model-roles.rerank]`; absent means retrieval keeps the fusion order.
_Avoid_: "judge" for this stage (a judgement model is a criteria-based verifier, which HAI does not run); "reranker" for the type (that is the model)

**soul**: the identity and voice layer — how the agent speaks, as one piece of free-form prose.

**agents**: the operational prompt layer — interaction rhythm plus memory and topic maintenance. Not the identity or voice, and not this repository's own AGENTS.md.

**context messages**: the accumulated sequence handed to the LLM — rendered messages, the chapter-opening block, and tool call/result chains, appended per Turn and persisted in the snapshot. Message is the stored content; context messages are the accumulated LLM product.

**ContextMeta**: the chapter's persisted scalars — context tokens, successful Turn count, Step count, and the chapter's session id — reset on reopen, which mints a new session id.

**conversation preference**: a per-Conversation setting that overrides the configured default — language, model alias, content policy, persona; no row means the Conversation inherits the default, and the stored blob tolerates fields a newer build added. `agent/settings.rs:ConversationSettings` resolves a preference and the configured default into the value a Conversation runs with.

**model alias**: a name in `[models]`, mapped to a `provider/model` entry and resolved at startup; `/model` selects one per Conversation. No alias means `[model-roles.default]`.
_Avoid_: "model role" for this catalog entry

**content policy**: the prompt-only axes a Conversation runs with — maturity (`safe`, `standard`, `mature`) and politics (`restricted`, `unrestricted`). Presets are the typed `config/conversation/content_policy.rs:ContentPolicyPreset` — `safe`, `standard`, `mature`, `unrestricted` — and any other spelling is an unreadable argument, not a silent default; a stricter change reopens the chapter through the existing wrap-up. The `unrestricted` preset adds no application-side restriction and tells the model not to refuse; a provider's or platform's own block remains outside the application.

**persona**: a user-set overlay above the Default role's soul, governing voice and style only; it cannot override the interface contract, tool rules or content policy. Wrap-up and digest passes do not read it.

**steering**: ending the current Turn early when a new event from its origin Chat arrives, then opening a new Turn to handle it; another Chat's event queues. Attention continuing, not work being preempted.

**attention window**: the spell during which the agent keeps attending — opened by a change addressed to it, or by its own reply — and inside which everything pending dispatches (`agent/chat/scheduler.rs:EventScheduler::on_addressed`, `:on_spoke`).
_Avoid_: bare "window" (that one is the context window), "attention span"

**Chapter wrap-up**: the pass that condenses a chapter into the next chapter's opening. It does not own topic lifecycle, runs with the same agent and tool surface as a normal turn, and its text output is machine-extracted.
_Avoid_: "summarize", "digest" (a summary is a conversational reply)

**ChatEntry**: one change inside a Conversation's accumulation, naming its origin Chat — a message created, a message edited, a command, a host action, a stricter content-policy change, a scheduled task, a confirmed purge — carrying what happened and when, never the content (the message row holds that). Entries live in `conversation.pending`, ordered by when the change happened.
_Avoid_: "event" (the `event` table is the agent's audit trail)

**accumulation** (`pending`): the changes of a Conversation that no Turn has consumed yet. A Turn takes the whole accumulation and consumes it only on a normal end; a failed Turn leaves it for the next attempt, and a generation start re-feeds it.

**WakeEvent**: what a Turn reads its changes as — one reason per accumulated change, coalesced into the situation text the model sees. A `Command` reason renders into the instruction the model reads.

**WakeReason**: how one accumulated change reads: Observe, Direct, Mention, Scheduled or Command. Who a change was aimed at decides it, and a private chat always reads as Direct. A **host action** and a confirmed purge answer no reason at all: the first is context the next Turn reads, the second a pass of its own.

**command table**: the agent's declaration of every command it offers — one row per command in `agent/command/spec.rs:DECLARATIONS`: wire name and aliases, menu copy (`agent/command/menu.rs`), the positionals and long options it accepts (`agent/command/args.rs`), the parse that turns them into a `Command`, and the complete examples its `--help` shows. Every row has one complete command line; a page of buttons only collects the same arguments and never replaces the line. A platform with a menu advertises the rows and merges its own declarations in front of them (`/start` on Telegram); TeamSpeak has no menu and reads the same rows behind `!`.
_Avoid_: "command list", "registry" (the agent registry is a different thing)

**command declaration**: one row of the **command table** — what a platform advertises in its menu, where it has one, and what the agent resolves a typed name through. A platform's own commands are declarations of its own (Telegram's `/start`), listed before the agent's.

**host operation**: a command the host answers itself, without handing the command to the agent (`/start`, `/status`, `/lang`, `/model`, `/content-policy`, `/persona`). It writes no message. One that changes a Conversation setting records a **HostAction** — context for the next Turn, never a reason to start one — and one that only reports records nothing.
_Avoid_: "built-in", "admin command"

**HostAction**: the typed record that a host operation changed one Conversation setting — persona, model, language or content policy (`domain/vo/host_action.rs:HostAction`). It says what moved, never to what: the new value is read from the Conversation's settings snapshot, so the accumulation never carries a second copy of mutable state. It is an entry of `EntryKind::HostAction` with audience `Context` (`EntryAudience::Context`) — silent, persisted in the accumulation, rendered as `<host_actions>` by the next normal Turn and consumed when that Turn ends normally. It reports something the host already did; it is not an instruction the agent must carry out and not a reason to dispatch.
_Avoid_: "AgentCommand" (an instruction from the person that a Turn acts on), "event" (the audit trail), "notification"

**agent task**: a command that is a conversation with the agent — it joins the Conversation's accumulation as a command entry, so it survives a crash, and is answered in the conversation (`/explain`).
_Avoid_: "task" (a scheduled task is `WakeReason::Scheduled`)

**host copy**: a reply the host composes itself instead of the agent, in the Conversation's language — a variant of `agent/host_copy.rs:HostCopy` and its `render`. A dialog's pages and a pass's receipt are host copy as well: they are about the host's own operation, not something the agent said, and live beside the command that says them (`agent/command/<command>/copy.rs`).
_Avoid_: "template", "i18n string"

**purge**: one action that wipes a Conversation's own traces over a **window** — the agent's records, the workspace files, every member Chat's platform messages, and the derived data — after the person confirmed it. It is deterministic: statements and file operations, no judgement by a model. The Conversation's own ConversationAgent carries it out in a pass of its own, never inside a Turn.
_Avoid_: "delete" (a message being removed on the platform is not this), "cleanup", "session reset"

**surface**: one of the four independent things a purge can touch — the agent's records (`agent`), the workspace files (`files`), the platform's messages (`telegram` on the wire, whichever platform the Chat runs on), and the derived data (`derived`: event rows, transcripts, and the cached bytes of attachments) (`domain/vo/purge.rs:PurgeSurface`). A purge that leaves `telegram` out forgets the conversation while the platform still shows it.

**window**: the stretch of time a ranged statement narrows by — both ends optional, a missing one unbounded on that side, the upper end inclusive (`domain/vo/time.rs:TimeRange`). What a **stretch** resolves to at the moment it runs.

**stretch**: how a person names a piece of time — everything, a span back from now, or two local dates covering whole days with the last one included (`domain/vo/stretch.rs:Stretch`). It resolves to a **window** only when the action runs, which is what lets a dialog hold a choice across a restart; `/purge` and `/digest` both name their time this way. A span is named in the largest whole unit that divides it (`domain/vo/stretch.rs:whole_units`), so a five-minute window never reads as zero hours, and the chat reads every span through `agent/host_copy.rs:stretch_text` alone — a range button, the confirmation it leads to and the receipt are the same words.

**confirmation**: the pages of host copy a command goes through before it acts (`agent/command/purge/dialog.rs`, `agent/command/digest/dialog.rs`, put out through the shared engine `agent/command/dialog.rs`) — held in memory, bound to the chat, and good only until it expires. Who may press is the page's own policy (`agent/command/dialog.rs:Audience`): a page that acts on the person who asked (`/purge`) admits only them, while a page that acts on the chat (`/digest`) admits anybody in it. A page that is gone changes nothing, and a press the page does not admit is refused.
_Avoid_: "wizard", "prompt" (a prompt is what the model reads)

**Skill**: a specialist capability described by a SKILL.md file; the prompt lists the names and the body is loaded with `read skill://<name>`.
_Avoid_: "plugin"

### Platform domain

**channel**: one configured platform instance and its in-process runtime — the agent's presence on one outward surface, with its identity (`ChannelProfile`, `ChannelKey`), what that surface supports (`ChannelCapabilities`), and the way in and out (`agent/channel.rs:Channel` out, `ChannelAccess` in). One platform implements one `ChannelAdapter` per instance, and the agent layer holds the outbound `Channel` and never a platform type.
_Avoid_: "ChatType::Channel" (a chat kind, not this); "bot" where the thing is literally a bot; conflating with ConversationAgent or with the platform itself

**channel handle**: one chat on that channel, as the agent acts on it (`agent/channel.rs:ChannelHandle`) — the chat is bound where the agent is created, so a send, a typing indicator or a platform file already know which chat they mean and no caller pairs a chat id with a channel. What the chat itself does not decide (`key`, `profile`, `capabilities`) it forwards to the channel.
_Avoid_: "guard" (a guard's meaning is its lifetime — that is `TypingGuard`); "handle" for the channel itself, which is `Channel`

**channel key**: the `platform:key` spelling one channel instance is stored under — prepared once where the channel is built, read by `platform_channel`, the scheduled-task scope and the log labels; a Chat points at the row by `channel_id`. An identity travels as a typed value; the spelling exists for the columns and the logs.
_Avoid_: "channel id" (that is the platform's own instance identity), "instance name"

**platform declaration**: what one platform offers the agent (`agent/channel.rs:ChannelCapabilities`) — the forms it can send and what each form takes, whether a message may point at another one or quote a part of it, and the upload ceiling. The code that must vary with the platform reads the declaration; a difference with no reader is not declared. A page of buttons is not a flag here: it is the separate `ChannelChoices` surface, present only on a platform that implements it.
_Avoid_: "capability list" (only the forms are a list), "platform config" (that is the `[channel.<name>]` section)

**channel adapter**: one platform's own half of the platform layer (`platform/adapter.rs:ChannelAdapter`) — the key its channel is stored under, its **platform declaration**, reading its updates into an **arrival**, sending what the agent asked for and answering with a receipt (`platform/adapter.rs:SentMessage`), starting a **typing guard**, file access, and deleting messages the chat no longer wants. Only the platform layer names it; the agent speaks **channel** on the way out and **channel access** on the way in.
_Avoid_: "transport" (that is HTTP or the vendor's client), "wire" (our own word for it), "adapter" for the layer as a whole

**arrival**: one update a platform read off its own update stream and handed to the inbound tail (`agent/inbound/arrival.rs:Arrival`) — chat (address, kind, title), sender, text, attachments, external id, reply-to, the change kind with the platform's own times, who it was aimed at, the platform's own message meta, and what the platform made of it (`Ordinary`, its own `Greeting`, a `Command` with its name and payload, or a `Choice` — one of our own buttons pressed, carrying its token, the platform's answer handle and the message the buttons hang on). Platform ids are strings here; the tail resolves them to rows. It carries no platform-shaped field of its own: whatever a platform knows beyond this set rides in its `meta`.
_Avoid_: "update" (that is the platform's own payload), "event" (that is the agent's audit trail)

**chat address**: the platform's address of one chat (`domain/vo/chat_address.rs:ChatAddress`) — an optional **origin** (the server that allocated a server-local id, such as a TeamSpeak server UID) and the platform's own external id. A centralized provider leaves the origin absent because its id is already global; the HAI channel that owns the chat is separate (**channel key**).
_Avoid_: "chat id" for the address when the origin is meant; "server" for the HAI channel

**channel access**: the inbound path's narrow handle on the platform (`agent/channel.rs:ChannelAccess`) — whether a chat may talk to this channel, where a private message to one sender goes, and how to deliver host copy or a host file. The platform's page of buttons is its own surface (`agent/channel.rs:ChannelChoices`), implemented only where it exists. `platform/adapter.rs:ChannelAdapter` declares `ChannelAccess` as a supertrait, so the tail reads the core methods and no vendor type; a platform that also implements `ChannelChoices` answers for that surface separately.
_Avoid_: "channel" bare when only the inbound half is meant

**typing guard**: the live typing indicator, held as a value whose lifetime is the indicator's (`agent/channel.rs:TypingGuard`) — dropping it ends what the platform started. How it is kept alive between those two points is the platform's fact, so the agent never names a cadence; a platform with no indicator answers `None`.
_Avoid_: "typing flag" (nothing declares whether a platform has an indicator — it answers with a guard or with nothing)

**inbound tail**: the one path an **arrival** takes after a platform hands it over (`agent/inbound/mod.rs:Intake`) — admission, the command fork, resolving the chat and its account, one-transaction ingest, and the doorbell. It is agent-owned: everything it reaches for is agent or domain vocabulary, so a platform only reads its own updates into an arrival and hands it over.
_Avoid_: "dispatcher" (that is one platform's routing), "platform intake" (the tail is not the platform layer's)

**ChannelHost**: the process-level host owning every channel — it starts them and orchestrates shutdown. What it keeps per channel is `platform/host.rs:ChannelRuntime`: the channel's `ChannelContext` (the adapter, the one inbound path, the ConversationAgents, and the token their shutdown hangs on) and the supervisor handle it stops.

**AppContext**: the process-level container holding config, settings, database services, the chat transport, multimodal and the event bus (`agent/process/context.rs`).

**MCP**: Model Context Protocol, the external tool-server protocol.

## Relationships

- One **Conversation** owns several **Topic** and several **Message**; a **Topic** consists of several **note**
- One **Conversation** owns one **ConversationRecord**, one **Workspace**, and one **ConversationAgent** (created lazily)
- One **Conversation** contains one or more **Chat**
- One **Turn** is bound to exactly one origin **Chat**; it contains several **Step** and produces several **Event**
- One **ConversationAgent** owns several **Turn** and may run several **chapter wrap-up**, each opening a new **Chapter**
- A **host operation** that changes a setting emits one **HostAction**; an **agent task** appends an `AgentCommand` — the first records what the host already did, the second tells the agent what to do
- One **Account** owns several **Memory**; a **Memory** belongs to one **Conversation** and may be attributed to one **Account**
- One **KnowledgeBase** consists of several **KnowledgeDocument**, each of several **KnowledgeChunk**, each carrying one **Collection** label
- One **ChannelHost** owns several **channel** and one process-wide **AgentRegistry**; a **ConversationAgent** reaches every member Chat's channel through one `ChannelRouter`

## Example dialogue

> **Dev:** "A message arrives and the ConversationAgent should wake — should the wake carry the message content?"
> **Domain expert:** "The change is written down, not carried: one transaction writes the message row and appends a ChatEntry to the Conversation's accumulation. What reaches the agent is a doorbell, and the agent re-reads the accumulation — one copy of the state, and only one thing to keep in step."
> **Dev:** "So what counts as one LLM call inside a Turn?"
> **Domain expert:** "That is a Step. A Turn is the whole loop — several Steps plus tool calls, until the agent stops. Calling them 'rounds' misleads the scheduler."
> **Dev:** "And those persisted TurnStarted/ToolCall records — run logs?"
> **Domain expert:** "Those are Events: observable agent-run events with a typed payload, consumed for auditing and recovery. Logs are tracing output."

## Flagged ambiguities

- **Conversation** owns context and state, **Chat** is a platform surface inside it, **ConversationAgent** is the agent entity, and **platform channel** is a configured connection — four layers, never interchangeable
- "summary" is a normal reply; the **chapter wrap-up** produces a retention summary that replaces history, and that summary is the next chapter's opening — a **Memory** is a separate stored entry, never the retention summary
- **AgentRole**, **ModelRole**, **model alias** and **persona** are four different things; never write bare "role"
- **AgentCommand** and **HostAction** both ride the accumulation and both name a Chat, but one is an instruction to act on and the other a record of something already done; a host action is never read as a reason to wake
- "knowledge" is ambiguous: a memory entry of kind knowledge is Conversation-scoped, while a **KnowledgeDocument** belongs to the global corpus
- Three unrelated retrievals: **WebSearch** (live web), knowledge-base search (curated corpus), **recall** (Conversation-scoped memory and topics) — name the tool, never bare "search"
- **need-close** is rendering semantics, and a closed topic stays editable
- "context" has four senses: the process container (`AppContext`), the per-turn carriers (`TurnContext`, `ToolContext`), and the prompt-rendering package — the per-turn carrier module is `turn`, not `context`
