# Session lifecycle and scheduling

> The event loop of a single chat: how an external WakeEvent comes in, how it is dispatched into a Turn, how a Turn ends (three states), and when the chapter reopens/exits. Required reading when changing the scheduling and lifecycle parts of `agent/runtime/session/`, `agent/runtime/event/`, and `runtime/run.rs`. The authoritative terminology is in docs/CONTEXT.md (Turn/Step/steering/context messages/ContextMeta).

## Overview

Manages the event-loop lifecycle of a Chat (`ChatId`): **Inbox receives a platform wake → EventScheduler decides when it should run → dispatch assembles context and spawns a Turn → the react loop executes → on_complete writes back the conversation and snapshot → on idle timeout or token over-threshold the chapter reopens (wrap-up stages first for retention) → exit**. Everything under `agent/runtime/` about "when/how to dispatch and the lifecycle of Turn and chapter reopen" lives here; the single-turn execution semantics of the react loop are in `agent/runtime/react.rs`, and the tool side is in topics/tools.md.

## Design

### The state machine has only two states, Idle/Busy; being in wrap-up is not a third state

```txt
Platform → SessionHandle.wake(WakeEvent) → Inbox.push + Notify
  → idle_tick (select! notified/status/deadline) → inbox.drain → schedule.enqueue → schedule.decide
    → Ready(events) → dispatch → spawn_turn/spawn_wrap_up → Busy
    → Defer → Idle; Done → chapter non-empty ? reopen (wrap-up) : Exit
Idle ──dispatch──→ Busy{handle, result_rx, started_at}
Busy ──Success/Steered/WrapUpDone/TurnFailed/WrapUpFailed/Cancelled──→ Idle (no exit; Exit only happens at Done + empty chapter)
```

`session/mod.rs:SessionState` has only `Idle | Busy{handle, result_rx, started_at}`; `take()` = `mem::replace` to Idle. **Being in wrap-up is one variant of Busy**; the event loop distinguishes them via the source of the `BusySignal` received on `result_rx` (`runtime/types.rs:BusySignal`: `Turn / Steered / WrapUp / TurnFailed / WrapUpFailed(error)`).

### Turn three states: Success / Steered / Failed

\*\*Turn is an atomic execution unit\*\*: from pulling messages to finishing is a complete closed loop; the cursor / chapter meta only advance on normal end (Success/Steered):

| Outcome | Trigger | Behavior |
|---|---|---|
| Success | skip / direct output | `on_complete`: advance context messages + ContextMeta + cursor commit + mark_seen + snapshot persist |
| Steered | new event during Turn (steering) | **ends early and normally** — everything already processed takes effect (same advance as Success) → immediately dispatch a new Turn with the interrupting event (incremental continuation) |
| Failed | react-loop error | **zero state side effects**: staged cursor discarded, no context messages written, no meta advanced, no mark_seen — the next Turn re-pulls the full range (A-B messages not lost) |

- "Status query received while Busy" need not be a separate state: in the Busy branch of `event_loop.rs:AgentSession::run`, `select!{biased; status_rx → Status, result_rx → …}`, and after replying it restores Busy as-is
- Failure variants are distinguished by source: a Turn failure only keeps Idle; a wrap-up failure reopens a clean chapter (see "Chapter reopen")
- Exit only happens at `Decision::Done` + current chapter empty; Turn/wrap-up failures stay Idle and do not exit — after the environment exits the registry is cleaned immediately, and on message wake it is rebuilt and loads the snapshot

### steering: new events during a Turn = attention continuation

While a turn is running and Inbox receives new events → after each step ends `react.rs` detects (main mode only; the `ReactRun.steering` toggle comes from the `[agent.context] steering` config key, default true):

- inbox empty → continue to the next round
- **debounce window** (new Turn start + 1500ms, `react.rs:STEERING_WINDOW`) with only Observe → do not interrupt (event put back, re-checked next round) — prevents Observe livelock in active groups
- otherwise (any event outside the window / non-Observe inside the window, e.g. Mention/Direct/Scheduled/Command) → return `steered: Some(events)` → `BusySignal::Steered` → event_loop: `on_complete` (advance + persist) → **immediately** dispatch a new Turn with the interrupting event + the rest of the inbox (incremental fetch, do not re-process already-seen messages)

**wrap-up does not respond to steering** (`run.rs:spawn_wrap_up` sets `steering = false`): a restricted react-loop (tools only organize topic/memory, no send_message), events stay in the inbox and are handled after wrap-up completes. The Turn input range stays complete — context is not modified mid-way.

### The scheduler is a pure timing engine (no side effects)

`session/scheduler.rs:EventScheduler` (holds the single pending queue) + `session/attention.rs:Heat/Window` only make timing decisions; they do not touch the DB or the platform. The scheduling strategy when an event enters is classified by `WakeReason` (`event/wake.rs:WakeReason::is_addressed / is_rapid / is_mergeable`):

| Method | Trigger condition | Effect |
|---|---|---|
| debounce | non-rapid event (Observe) | wait 1500ms after the last event before it may dispatch; merges bursty message streams |
| Heat spend | `random < heat.value` and queue non-empty | probabilistic dispatch ("random attention"; an empty queue produces no empty turn, see pitfalls) |
| Window | is_addressed (Direct/Mention/Scheduled/Command) | refresh window + heat reset to 1.0 (MAX_HEAT); all events in the window are dispatched |
| last_activity | **only addressed events / agent speech (`refresh()`)** | idle timing baseline (Observe is not attention, does not refresh) |
| timeout | `max(window close, last_activity) + idle_timeout` | **always Done** (does not dispatch backlog — Observe backlog is handled by the window/heat normal path; message content is guaranteed by the DB + since_id cursor) |

Decay: no decay before the window closes; after that, one step every 60s, the part above base is halved exponentially by `0.5^steps` (`attention.rs:Heat::decay`). base comes from `config/schema.rs:AttentionConfig.base_attention` (default 0.05); `window_secs` defaults to 30.

### Scheduled-task expiration wake

Scheduled tasks (the `scheduled_task` table + the `schedule_task`/`list_scheduled_tasks`/`cancel_scheduled_task` tools, see tools.md) are woken on expiration by a **per-bot resident watcher**: `platform/telegram/scheduled_watcher.rs:ScheduledTaskWatcher` (brought up with each bot by `builder.rs`) polls `due(bot_id, now)` every 60s → `registry.get_or_create(chat)` → `handle.wake(WakeReason::Scheduled(TaskPayload{task_id, description}))` — **isomorphic to platform event injection**, going through the existing scheduling/chapter-reopen path rather than adding a new wake channel. After a periodic task fires, `advance` moves `fire_at` forward (skipping past-due times, taking the next future trigger point — even missing several times only triggers once); one-off tasks are deactivated. The audit trail reuses the event log; no execution table is created. `Scheduled` is an addressed event (`is_addressed`); it triggers a turn at the appointed time and the bot decides how to respond.

### Chapter reopen + wrap-up staged-first retention (core mechanism)

**Idle reopen of the session is the main body; wrap-up is just a pre-retention program**: as long as no attention window period is triggered again beyond `idle_timeout` (5 minutes), the chapter reopens — the session is clean **and does not depend on wrap-up succeeding**; wrap-up is only attempted once, on success the summary is placed at the start of the new chapter, and on failure a clean chapter is still opened. Two reopen trigger points:

1. **idle expiry (trigger ②)**: after `Decision::Done`, go through `event_loop.rs:should_wrap_up` — **semantic determination of `conversation.has_unwrapped_content()` (chapter `turn_count > 0`)**; after reopen `turn_count` resets to zero (`start_new_chapter` replaces the chapter wholesale with `Chapter::new()`) → naturally prevents duplicates
   - **overdue restore variant**: when restoring a session, if `conversation.updated_at` (last save ≈ last activity) has already exceeded `idle_timeout` and the chapter is non-empty → `idle_tick` reopens on the first tick (`take_restored_last_active` consumed once), without waiting for a new idle
2. **token threshold (trigger ①)**: `event_loop.rs:should_wrap_up_by_tokens` — `ContextMeta.tokens >= config/schema.rs:ContextConfig.compact_token_threshold` (default 150000, 0 = disabled; prevents context overflow causing hallucination or exceeding the model window) triggers at idle, **`idle_tick` checks this first**; on failure it also reopens (tokens reset to zero, so it won't immediately trigger again)

Wrap-up execution form (`runtime/run.rs:AgentRuntime::spawn_wrap_up`):

- `ReactRun::new(…, 0)` → **turn_number=0** (wrap-up step/tool events are marked with `turn: 0` — the project's custom convention; wrap-up lifecycle is identified by the separate WrapUpStarted/Completed/Failed events)
- **`steering = false`**: wrap-up does not respond to steering; events stay in the inbox and are handled by `drain_into_idle`
- injection method (community consensus — end-of-session summary = a task-style instruction): system preserves the conversation identity (`build_react_config` output), and the `runtime/run.rs:WRAP_UP_PROMPT` task instruction is appended **as a user message** (organize memory/topics → output a retention summary, structurally constrained by the `<summary>` tag + time range / key facts / user preferences / open items — **contains timeline information**: the summary is a summary of historical conversation; on restore, old vs new events must be distinguished; time ranges + key facts are annotated in chronological order, using the message rendering `<date>` / `<msg at>` time to judge); **must output the summary text before ending** (no skip on the tool side, it cannot be "skipped")
- tool subset `tools/mod.rs:get_wrap_up_tools`: only topic/memory (**no skip** — wrap-up is a background task, cannot be skipped, must output a retention summary)
- stopping semantics `react.rs:LoopMode::WrapUp`: **stops on no tool call** (multi-round organization — the model's first-round organization-declaration text is not truncated; plain text = final summary); summary extraction: prefer the `<summary>` tag (take the most recent step in reverse order), fall back to the **most recent non-empty step text** when there is no tag (including early text such as the organization declaration); only when every step is empty text is it judged WrapUpFailed; round limit (MAX_STEPS 20) prevents the model from calling tools forever without outputting text
- summary extraction: prefer the `<summary>` tag content (prompt structural constraint), fall back to a non-empty reply when there is no tag; **no length threshold** (quality relies on instruction and structural constraints — the community has no length-filter practice); none → `BusySignal::WrapUpFailed(error)`

Reopen actions (`event_loop.rs`):

- **success** (`on_wrap_up_done`): take the pre-wrap-up step count → emit `WrapUpCompleted { step_count }` (CLI shows `WRAP N steps`) → `start_new_chapter(Some(summary))`: summary placed at the start of the new chapter, **ContextMeta reset to zero (turn_count/step_count/tokens), shown ids cleared** → snapshot persisted; **since_id unchanged** (conversation-level cursor crosses chapters)
- **failure** (`BusySignal::WrapUpFailed(err)`) → emit `WrapUpFailed{error}` event → `start_new_chapter(None)`: **open a clean chapter** (context messages/ContextMeta/shown all cleared) + snapshot persisted + back to Idle — no backoff, no retry (prevents the old implementation's "no summary → backoff → retry → infinite loop")

### ContextMeta: chapter-level scalar aggregation (replacing turns persistence)

`domain/vo/context_meta.rs:ContextMeta { tokens, turn_count, step_count }` — a chapter-level persistent scalar describing "the state of the current context":

- `tokens`: the prompt tokens of the last successful Turn (taken from `steps.last().prompt_tokens` on `conversation.rs:Conversation::update`; the over-limit trigger criterion, no longer recomputed from turns)
- `turn_count`: number of successful turns in the chapter (advanced by Success/Steered; reset to zero on reopen — **chapters are independent units, event numbering restarts per chapter**)
- `step_count`: chapter scale (status display / WrapUpCompleted.step_count)

**turns are not persisted**: `Turn` is a runtime type (accumulated in the react-loop → emit events / extract summary / scan has_spoken → discarded); auditing is handled by the events table (TurnStarted/TurnEnded/StepCompleted/ToolCall/ToolCallResult all persisted).

### First-turn determination = chapter initial state (turn_count == 0)

`conversation.rs:Conversation::is_first_render()` = `chapter.meta().turn_count == 0` — a chapter that has never successfully run a turn = initial state → full build. **A new chapter from any source satisfies this**: brand-new conversation / wrap-up success (summary at start, context non-empty) / wrap-up failure opening a clean chapter — the first turn is always fully built; non-first (turn_count > 0: continuation/restore seamless) → incremental. **The `next_first_render` flag and `is_fresh` cursor determination were removed**; a failed turn has zero state side effects (does not advance turn_count) → first-turn eligibility is naturally preserved.

### Conversation snapshot layer boundaries

In-memory state `conversation.rs:Conversation` (context messages + `chapter.rs:Chapter{ContextMeta, shown ids}` + since_id + cursor staging + restored_last_active) → `domain/vo/conversation_snapshot.rs:ConversationSnapshot` → `domain/service/conversation_record.rs:ConversationRecordService`. After on_complete / reopen, take a snapshot and persist; restore goes through `from_snapshot` to rebuild. The persistence format is decoupled from the in-memory structure (dual-channel persistence background).

**Context message persistence**: the accumulated message sequence given to the LLM (including injection/tool chains) is persisted — restore = seamless continuation (load chapter with turn_count > 0 → non-first → incremental continuation), "this conv" is not lost; the token threshold (150000) bounds the size (cleared on reopen) → storage is bounded.

### since_id cursor: stage-commit, only advances on normal end

`dispatch.rs:AgentSession::gather_messages` is driven by **first-turn determination** (no longer is_fresh):

- first turn (chapter initial turn_count == 0): `domain/service/message.rs:get_context_messages(chat_id, context_seed_cap)` (default 10, configured by `[agent.context] context_seed_cap`) — **pull all unread messages** + backfill history when below seed cap to fill up (unread in reverse order + backfill history when insufficient, returned ascending); **empty chat → last_id = -1**
- non-first: `get_messages_window(chat_id, Some(since_id))` (`id > since_id` full reverse then reversed), next_id = the last message ID (no new message → reuse since_id)

Cursor advance: after the dispatch pulls, `stage_since_id(next_id)` (staging) → on Success/Steered commit it in `on_complete` (`commit_since_id`); on Failed discard it (`discard_since_id`) — \*\*messages pulled by a failed Turn stay unprocessed; the next Turn re-pulls the full range (A-B messages are not lost)**. `start_new_chapter` does **not reset** since_id (conversation-level cursor crosses chapters).

### mark_seen only marks unmistakably covered messages

After a Turn ends normally (Success/Steered both go through the Ok branch of `runtime/run.rs:spawn_turn`), `run.rs:mark_seen(payload.message_ids)` → `db.message.mark_unread_seen(ids)` (only rows with `interaction_status=unread`) — only marks the messages actually fetched and processed by this Turn's gather. **Does not pre-mark messages that arrive during the Turn**: under the new execution model they are handled by subsequent Turns (Steered continuation/incremental); pre-marking seen would lose the "new messages" separator line for rendering (`<separator>` is based on unread status). Failure paths do not mark (messages stay Unread, re-pulled next time).

## Boundaries

- Sessions (execution environment) can be rebuilt; after idle exit the registry is cleaned immediately; conversations (persistent state) live forever
- Chapter reopen is driven by only two trigger points (idle expiry / tokens ≥ 150000); no startup scan (chats with no new message after restart are not reopened; the interface is reserved)
- Environment exit only happens at `Decision::Done` + chapter empty; Turn/wrap-up failures do not exit
- turn numbering is chapter-level (reset to zero on reopen); wrap-up does not occupy a number (event types already distinguish it)
- unread fetch has no limit (message visibility takes priority; a known extreme-scenario context overflow risk)

## Pitfalls / common mistakes

- heat hits and queue empty **no longer** returns Ready(empty) (`session/scheduler.rs:EventScheduler::decide` fix); idle expiry **always Done** (does not dispatch backlog) — Observe backlog is only handled by the window/heat normal path
- after first-turn empty chat, since_id=-1 (`dispatch.rs:gather_messages` → `message.rs:get_context_messages` empty → last_id=-1), later goes through `id > -1` to pull everything — an empty session also pulls fully, and the longer the history the bigger the overhead
- has_spoken is computed twice in `dispatch.rs:on_complete` and `run.rs:spawn_turn` (TurnEnded emit) (same logic) — changes need to be synced in both places
- turn lifecycle events (TurnStarted/TurnEnded/StepCompleted/ToolCall/ToolCallResult) are emitted inside the spawn_turn task, **not at the session layer** — when tracing event ordering, do not look in session/dispatch
- wrap-up and turn have different BusySignal failure variants (TurnFailed vs WrapUpFailed): a turn failure only keeps Idle (cursor discarded); a wrap-up failure **reopens a clean chapter** (`conversation.rs:Conversation::start_new_chapter(None)`, no backoff, no retry) — don't handle them uniformly when adding a failure path
- `should_wrap_up` uses `has_unwrapped_content()` (chapter turn_count > 0) for its determination, not message_count/is_first_render — before changing the reopen criterion, confirm the two boundaries of a restored session and a reopened one (meta reset to zero, since_id preserved)
- steering detection only applies to the main Turn (`LoopMode::Main` + `steering` toggle); wrap-up is exempt — adding interrupt logic to wrap-up would break retention
- within the debounce window (STEERING_WINDOW 1500ms) only Observe does not interrupt; Observe outside the window also interrupts — the window's job is to prevent active-group livelock, not a permanent exemption
- idle expiry with an empty chapter → environment **exits** (registry cleaned immediately); Turn failures **no longer exit** — "exit" only happens on the idle path; when a session disappears, first distinguish idle exit from creation failure (on restore, DB violations record an error and do not build a session)
- leftover: `event/inbox.rs:Inbox::len` is used by the react.rs steering detection (no longer dead code); the `select!` `else => return NextStep::Exit` only triggers when status_rx is closed and there is no deadline (a normal path of a session about to be destroyed) — don't rely on it for timeout exit

## Evolution direction

- startup scan: after restart, automatically reopen chats that have "no new message but a non-empty chapter" (spec reserves the interface; needs a background scanning component + throttling)
- unread fetch cap: extreme scenarios (long-time-offline message pile-up) context overflow — message visibility takes priority, so no cap for now; future may truncate by priority
