# Telegram Platform Integration

> Message entry, internal commands, send chain, media analysis. Must-read when changing `platform/`.

## Overview

Telegram is the only platform implementation. Responsibilities: receive messages → persist to DB → wake the session; send the agent's replies/voice back to the platform; download attachments and perform multimedia analysis. Platform details are shielded from the agent layer by two traits (PlatformHandler/ContentParser).

## Design

### Module responsibilities (platform/telegram/)

| File | Responsibility |
|---|---|
| builder.rs | `TelegramPlatform::spawn`: Bot::new → TelegramPlatformHandler::new → SessionManager::new → TelegramDispatcher::new → ScheduledTaskWatcher::spawn → tokio::spawn |
| handler.rs | **PlatformHandler impl** (10 methods) + **bootstrapping identity** (ensure_bot_account + get_me + get_my_name); send fallback chain |
| dispatcher.rs | teloxide routing: whitelist filter → filter_command → handle_command; fallback handle_message; command handling inlined |
| command.rs | `Command` (BotCommands: Start/Status/OrganizeMemory/Explain/Digest(u32)) + `parse_digest_days`; only defines the enum, does not process |
| scheduled_watcher.rs | `ScheduledTaskWatcher::spawn`: per-bot resident loop (every 60s) polls `due(bot_id, now)` to wake the session (mechanism: see docs/topics/session.md "Scheduled task expiry wakeup") |
| message_handler.rs | resolve_chat_and_account / persist_user_message / dispatch_agent_event (WakeReason derivation) |
| parser.rs | `ContentParser` impl: DB content JSON → ParsedContent + renderer |
| render.rs | `render_content`: TelegramContentPart → Node (attachment + `<analysis>` child node + same_resource_as) |
| service.rs | `download` / `get_file_url` / `send_rich_message` (raw HTTP POST, custom API outside teloxide) |
| media.rs | `download_attachment` / `analyze_part` / `persist_analysis` / `download_file_cached` |
| util.rs | `escape_md_v2` + parsing and escaping helpers (msg_chat_type / is_mentioning_user / ExtractedTelegramMessage) |

`platform/manager.rs:spawn_bots`: iterates `cfg.bot` → `BotConfig::resolve` → `TelegramPlatform::spawn`; the JoinHandle is dropped with `let _handle =` (bot tasks detached, a crash only leaves a log; any spawn failure → the whole startup fails).

### Message entry flow

```txt
dispatcher.run() (Update::filter_message)
  → is_allowed_chat filter (false → only /start replies "You are not authorized…")
  → filter_command::<Command> → handle_command (not via agent)
  → fallback handle_message(msg, me):
      msg.from (none → ignore) → msg_chat_type
      → resolve_chat_and_account (ensure_chat_and_account)
      → persist_user_message (reply_to reverse-looks-up the internal ID; ExtractedTelegramMessage::extract → save_user_message)
      → dispatch_agent_event: Private → Direct; is_mentioning_user → Mention; otherwise Observe
      → session(chat_id).wake(WakeEvent::new(reason)) — records an error on DB violation, does not create a session
```

**Persist first, then wake**: persistence happens in the parsing layer, consistent with "WakeEvent carries no content" — the session later pulls via gather_messages(since_id). Command input (persist_user_message before handle_command responds) is **likewise persisted first** — so the agent context can see what commands the user issued.

### Internal commands (inlined in dispatcher)

| Command | Behavior |
|---|---|
| `/start` | "Hello!" (unauthorized: warning + only replies the authorization notice) |
| `/status` | `session(chat_id).status()` → model/steps/tokens/conv/heat/window text |
| `/organize_memory` | `wake(WakeEvent::new(WakeReason::Command(AgentCommand::OrganizeMemory)))` |
| `/explain` | `wake(WakeEvent::new(WakeReason::Command(AgentCommand::Explain)))` |
| `/digest [N]` | `wake(WakeEvent::new(WakeReason::Command(AgentCommand::Digest(days))))` (default 7 days) |

### Send chain (handler.rs PlatformHandler impl)

- `send_message`: rich_message → `send_rich_message` (raw HTTP) → on failure fall back to `send_with_markdown_fallback` (escape_md_v2 + ParseMode::MarkdownV2) → on failure fall back to plain text; persist `TelegramContentPart::Text`, returns SentMessageMeta{external_id}
- `send_voice`: `InputFile::memory(bytes)`, falls back to `tts_{Uuid v7}` when file_id is missing, content `Voice{meta: VoiceMeta{prompt}}`
- `send_typing`: ChatAction::Typing, on failure only an error log
- `download_file` / `get_file_url`: via `media.download_file_cached` (key = `"telegram-{file_id}"`)
- `analyze_attachment`: `download_attachment` → `analyze_part` → `persist_analysis`
- `bot_id()` / `profile()` / `content_parser()` → `&'static dyn ContentParser` / `message_capability()` → Rich or MarkdownV2

### Media analysis

```txt
download_attachment(uuid): message.find_attachment (scans the latest 200) → (part, file_id, attachment_parser())
analyze_part:
  Image+Sticker → download bytes, analyze_image(Bytes)
  Image → file_url + analyze_image(Url)      # URL direct-pass saves bandwidth
  Ocr → ocr(Url)
  Video → analyze_video(Url, media_format, focus)
  Audio → analyze_audio(Bytes, media_format, focus)
persist_analysis: PerceptionService.upsert(Source::platform("telegram", file_id), parser, focus, content)
```

Attachment content is rendered in the message: `render.rs` generates `<attachment id type>` + **two-layer merge** — base transcription (focus=None) `<analysis>` + targeted judgment (focus=Some) `<analysis focus>` child node + `same_resource_as` (the same file is only analyzed once).

## Boundaries

- Only Telegram is supported (BotPlatform is Telegram only; Platform::Qq is unused)
- Internal commands do not go through the agent, are not persisted, and have no session dependency (/status and /organize_memory are exceptions — they need a session)
- No automatic supervision of multiple bots (JoinHandle dropped, a crash only leaves a log)

## Pitfalls / common mistakes

- **Bot crash is unrecoverable**: `platform/manager.rs:spawn_bots` drops the JoinHandle with `let _handle =` — a crash only leaves a log and is not automatically restarted (you need to build your own supervision for multi-bot deployments)
- **Channel message semantics are collapsed**: `util.rs:msg_chat_type` only checks `is_private`, so Supergroup/Channel are all classified as Group
- **Unauthorized users only get the /start reply**: other messages are silently ignored (only a warn log) — when debugging "why messages get no response", first check the whitelist
- **escape_md_v2 retains formatting characters**: when AI output contains `*`/`_`/`` ` `` etc., MarkdownV2 parsing may fail — there is a fallback, but it fails once first and adds an extra network round trip
- **Mention judgment follows UTF-16**: the entity offset/length in `util.rs:is_mentioning_user` is computed in UTF-16 code units — `encode_utf16` first, then slice; indexing by char misaligns and may even panic
- Legacy: `find_attachment` full scan (phase 2 removed the 200-entry limit — very old attachments can be analyzed, but the full-scan cost grows with message volume). Cleanup done: dispatcher's `enter_dialogue + State(Start)` empty shell deleted, `MessageCapability::Plain` variant deleted
