# HAI Domain Language

HAI is a long-term companion AI assistant running as a Telegram bot. This document is the domain-language authority — it defines only what each word *is*, with no implementation details. Implementation details are in the spec docs (docs/topics/session.md, docs/topics/tools.md, etc.).

## Language

### Entity domain

**Chat**: a Telegram conversation (Private/Group/Supergroup/Channel); the basic boundary the agent runs in.
_Avoid_: "room", "conversation", "talk"

**Account**: a user's identity on a platform (currently only Telegram), carrying platform metadata.
_Avoid_: "user" (the user is the real person), "identity"

**Identity**: a cross-platform user identity, which will bind multiple platform Accounts in the future.
_Avoid_: conflating with Account

**Topic**: a set of related messages within a Chat, with Active/Closed states.
_Avoid_: "theme", "thread" (thread is a platform concept)

**need-close**: the archive hint attached to a Topic at render time (an XML attribute) — signals the model to call CloseTopic; purely rendering semantics, no DB automatic marker, no reopen capability.
_Avoid_: "closed", "archive marker" (archiving is the CloseTopic tool behavior)

**Memory**: a persisted knowledge entry (UserFact/Note/Knowledge), automatically vector-embedded.
_Avoid_: "memory fragment", "knowledge base", "note"

**Perception**: the cached result of multimodal analysis (unique key source+parser+focus; focus=None base transcription line / focus=Some targeted-judgment line, two layers).
_Avoid_: "analysis record", "perception data"

**KnowledgeBase**: the global (not chat-scoped) curated knowledge-document collection, managed via CLI, consumed only by the agent (RAG auto-retrieval + active retrieval).
_Avoid_: conflating with Memory — Memory is chat-scoped personal memory, KnowledgeBase is a global curated corpus; they coexist

**KnowledgeDocument**: one document in the knowledge base; the lifecycle unit (import/update/delete granularity), with title/collection/source.
_Avoid_: generic "document", "file" (file is a disk concept)

**KnowledgeChunk**: the retrieval unit after document chunking (each chunk independently embedded).
_Avoid_: "fragment" (fragment is a message-reference concept)

**Collection**: the sub-library label of a knowledge document (a string field on the document).
_Avoid_: "category", "directory" (directory is a filesystem concept)

**Message**: one message in a Chat (user/assistant), content as a TelegramContentPart fragment list.
_Avoid_: "message record"

**Event**: an observable event of an agent run, payload as the `AgentEventPayload` tagged enum (single-table persistence).
_Avoid_: "log" (log = tracing output)

**ConversationRecord**: the persisted snapshot in the `conversation` table (context messages + state{since_id} + context_meta{shown ids, tokens, turn_count, step_count} — state-metadata grouped in two columns, conversation-level/chapter-level lifecycle separated; turns and first-turn marker are deleted — first-turn determination = chapter-initial state turn_count==0).
_Avoid_: conflating with Conversation (runtime state container)

**Chapter**: the segmentation unit of conversation history — current chapter (ContextMeta{tokens,turn_count,step_count} + context messages + shown ids) + archived chapter (the most recent user(wrap-up) retention summary; old content physically discarded).
_Avoid_: "paragraph", "round" (round = Turn)

**Chapter reopen**: the behavior of opening a new chapter when the attention window is not re-triggered before idle_timeout (5 minutes) — wrap-up is only one part of it (preventing important-context loss; success or not doesn't matter — on success the summary is placed at the head of the new chapter). On session restore, if the last activity is overdue, reopen immediately (don't wait for a new idle).
_Avoid_: "compaction", "cleanup" (wrap-up is the retention step of reopen, not the behavior itself)

**Scratchpad**: one slot of temporary text storage per Chat (reserved feature — skeleton retained pending implementation, currently zero consumers).
_Avoid_: conflating with ConversationRecord

### Runtime domain

**Session**: one AgentSession per Chat, state machine Idle/Busy.
_Avoid_: "connection", "instance"

**Turn**: the execution unit in which the agent fully responds to one wake-up (react-loop, may contain multiple Steps and tool calls). Atomicity: the complete loop from message fetch to reply. Three-outcome: Success (normal completion) / Steered (interrupted by a new event, ends normally early — processed content takes effect, immediately incrementally continues a new Turn) / Failed (exception — zero state side effects, everything redoes). Turn numbering is chapter-level (reset on reopen).
_Avoid_: "round"

**Step**: one LLM API call (including that call's tool-call results); a component of a Turn. Step numbering per-Turn. **Runtime-recorded** (not persisted; audit is carried by the events table).
_Avoid_: "step" when it means a process description rather than an LLM call

**ModelRole**: a named reference to a model's use (`enum ModelRole { Vision, Audio, Tts, Embedding, ImageGen }`) — the main model (`[agent]`) is the default role. Config see docs/topics/config.md "Model roles (auxiliary)".
_Avoid_: "model config" (too broad) — role specifically means "select model by use"

**auxiliary**: non-main-conversation model-role blocks (Hermes-style structure) — omitting a block = delegate to the main model (understanding class) / capability unavailable (dedicated class). The role selects the model; capability params (voice/speed/dimension) belong to each block.
_Avoid_: "model-roles" (flat five-role naming; this project uses main model + auxiliary overlay)

**ModelRef**: `{provider, model}` model reference — provider omitted = `agent.provider` (url/key are in the `[providers]` section).
_Avoid_: bare-string model names (must go through ModelRef across boundaries)

**context messages**: the in-process accumulated message sequence given to the LLM — original message rendering + system/memory/topic/knowledge-base injection + tool call/result chains, incrementally appended per Turn. Persisted in the snapshot (size limited by token threshold, cleared on reopen). Contrast with Message (the DB content truth): Message is original content, context messages are the accumulated LLM product.
_Avoid_: "rendered messages" (old name), conflating with Message (DB message table)

**ContextMeta**: chapter-level persisted scalar aggregate: tokens (context occupancy = prompt tokens of the last successful Turn, the trigger-point criterion for over-limit) / turn_count (successful Turns in the chapter, chapter-nonempty determination) / step_count (chapter scale, display). Reset on reopen.
_Avoid_: "run-artifact"-style naming (the run term is deprecated; ContextMeta is chapter-level metadata, not a Turn result)

**steering**: the mechanism that ends the current Turn early when a new event (inbox non-empty) is detected during a Turn, then re-fetches messages and opens a new Turn. A new event during a Turn is a continuation of attention behavior (user typing messages one by one) — debounce only merges the input phase; Turn-phase real-time is guaranteed by steering. Trigger = any event during a Turn (Observe queued in a new Turn's debounce window doesn't interrupt; Mention/Direct/Scheduled interrupt immediately; any event outside the window interrupts).
_Avoid_: "preemption", "mid-insertion"

**Chapter wrap-up**: the final echo at the end of a chapter's lifecycle — organizes this chapter's memory/topic + outputs a retention summary (optionally successful), the summary as the opening of the new chapter.
_Avoid_: "summarize", "digest" (summary = a conversational reply)

**WakeEvent**: an externally triggered session wake signal (pure notification, reason only, no message content).
_Avoid_: "notification", "request"

**Inbox**: the Session's event input queue.
_Avoid_: "buffer"

**Handler**: a PlatformHandler trait implementation (TelegramPlatformHandler).
_Avoid_: "driver", "adapter"

**Skill**: a specialist capability described by SKILL.md (agent-skills parsing).
_Avoid_: "plugin", "knowledge base"

**Heat**: the base value of the probabilistic attention mechanism (base 0.05, spend 0.25, reset 1.0).
_Avoid_: "weight"

**Window**: the attention time window (30s active period after being @'d/replied to).
_Avoid_: "window period"

### Platform domain

**MCP**: Model Context Protocol, the external tool-server access protocol.
_Avoid_: "plugin"

**FileCache**: the disk file cache under data_dir/files.
_Avoid_: "cache" (cache is too generic)

## Relationships

- One **Chat** owns several **Topic**; one **Topic** belongs to exactly one **Chat**
- One **Topic** may carry a **need-close** hint at render time (archive signal, not a DB state)
- One **Chat** owns several **Message**; one **Message** belongs to exactly one **Chat**
- One **Account** owns several **Memory** (UserFact); one **Memory** may belong to one **Account** or one **Chat**
- One **Chat** owns several **Perception** (multimodal result cache)
- One **Chat** owns exactly one **Session** (lazily created)
- One **Session** owns exactly one **Inbox**
- One **Session** executes several **Turn**; one **Turn** contains several **Step**
- One **Turn** produces several **Event** (persisted via AgentEventBus)
- One **Session** may execute several **chapter wrap-up** (idle/threshold triggered, wraps up the current chapter)
- One **Chat** owns one **ConversationRecord** (persisted snapshot)
- One **ConversationRecord** contains one current **Chapter** (ContextMeta + context messages) + the retention summary of the most recent archived chapter
- One **chapter wrap-up** archives the current **Chapter** as a retention summary and opens a new **Chapter** (summary at the new chapter's head; new chapter initial state → first Turn fully built — first-turn determination = turn_count==0)
- One **Chat** owns one **Scratchpad** (reserved feature, skeleton retained pending implementation)
- One **KnowledgeBase** consists of several **KnowledgeDocument**
- One **KnowledgeDocument** consists of several **KnowledgeChunk**; one **KnowledgeChunk** belongs to exactly one **KnowledgeDocument**
- One **KnowledgeDocument** belongs to one **Collection** (string field, may be uncategorized)

## Example dialogue

> **Dev:** "A message arrives, the session should wake — should I carry the message content when I wake?"
> **Domain expert:** "No. WakeEvent is a pure notification — the session wakes itself and pulls from the DB. Carrying message_id introduces a second state sync, and preempt/mark_seen would both get tangled."
> **Dev:** "So what counts as one LLM call inside a Turn?"
> **Domain expert:** "That's a Step. A Turn is the whole react-loop — possibly several Steps plus tool calls, until skip. Writing 'rounds' in docs misleads the scheduler."
> **Dev:** "These TurnStarted/ToolCall persisted things — can I call them run logs?"
> **Domain expert:** "No, those are Events — observable agent-run events, payload a tagged enum, consumed for auditing and recovery. Logs are tracing output; different things."

## Flagged ambiguities

- Chat and Session are distinct: Chat is a Telegram conversation, Session is the event-loop instance — don't treat them as interchangeable
- "summary" = a normal conversational reply (not chapter wrap-up — wrap-up is a retention summary, user(wrap-up) replaces history)
- "Topic" is the uniform term; "thread" is a platform concept (don't conflate)
- Session state is only Idle/Busy (wrapping-up is a kind of Busy, distinguished by the BusySignal source in result_rx)
- "knowledge" is ambiguous: Memory.kind=knowledge is a chat-scoped memory entry; KnowledgeDocument is a global curated corpus — they coexist, not mutually exclusive
- "need-close" is purely rendering semantics (XML attribute); the DB has no automatic marker and no reopen capability
