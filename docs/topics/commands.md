# Commands

> The command table, who a slash message belongs to, and the menu. Must-read when adding a command or changing what one replies.

## Overview

A command is one row of `agent/command/spec.rs:DECLARATIONS` — its name and its aliases, its menu copy (`agent/command/menu.rs`), the positionals and options it accepts (`agent/command/args.rs`), and the parse that turns them into a `Command` (`agent/command/parse.rs`). The same row spells its synopsis, so the form a person is shown and the form the parser reads cannot drift apart. Nothing else holds that list: the menu, the argument parsing and the answering all read the row.

**The command line is the contract; a page is a UI.** Every command has one line that names everything it needs and reaches the action without a page. A platform without buttons loses the page, never the command: an unfinished line is answered with the full form. Each platform owns its adapter — deciding whose message a command line is, rendering a menu where the platform has one, sending a reply (`platform/telegram/`, `platform/teamspeak/`). Telegram reads `/`, TeamSpeak reads `!`; the command rows are the same.

## Design

### The declaration

Each identity answers these questions:

- `name` and `aliases` — the wire spelling. Names match **exactly**: the command contract does not inherit a platform's spelling habits, so `/explan` is `Explain` and `/Lang` is an ordinary message.
- `copy` — the menu copy, one entry per client catalog.
- `grammar` — the positionals and long options, in the order they are written; `agent/command/args.rs:Grammar::synopsis` renders the form from it.
- `examples` — complete payloads, written without the command name or the platform prefix; `--help` prints them behind the platform's prefix. A page-only form is never an example.
- `parse(payload) -> Invocation` — the invocation the payload denotes, and the only place a `Command` is built. `Invocation::Ready` carries the command itself, typed: an `AgentCommand` for an agent task, a `SystemCommand` for a host operation (`LanguageArg` lives there); `Invocation::Unreadable(ArgError)` carries why the payload does not fit the command; `Invocation::Help` is `--help`, answered with the same synopsis.

`find` resolves a name or an alias to a row. `/digest`'s own parser holds its range inside 1–365 whole days and refuses anything else with the reason (`agent/command/parse.rs`); a range nobody named is a question for a page, not a default.

### The payload grammar

Every payload is read through one GNU-style grammar (`agent/command/args.rs`); a command declares its positionals, its long options, and whether its first free token starts a raw tail:

- **`--name value` and `--name=value` are one argument.** The long kebab-case name is the only spelling; a flag that takes no value is written bare (`--yes`, `--reset`), and a value written for one is refused.
- **A bare token is a declared positional, in declaration order.** `purge 7d --surfaces agent,files` is the only spelling, and a second bare token is an extra one.
- **`--` ends option parsing.** A command with a raw tail (`explain`) takes everything after its first free token verbatim; `--` is how text that starts with a dash is written (`explain -- --verbose`).
- **`-h` and `--help` are universal.** There are no short options and no abbreviations.
- **A refusal names the argument and the reason** (`ArgError`: unknown option, missing or unexpected value, duplicate, conflict, extra or missing positional, unreadable value, out of range), and `agent/host_copy.rs:HostCopy::BadArgs` renders it in the Conversation's language and appends the synopsis, so the form is repeated wherever the line failed.
- **A flag that appeared is read by presence, never by emptiness** — `--reset` holds nothing and is still given. A command's own cross-checks (an argument that cannot come with another) run after the payload is read.

### Who a message belongs to

`platform/telegram/inbound/command.rs:classify` answers **ownership**, independently of recognition: `/` at the start is a command line, a head whose `@name` is another bot is `Theirs`, and a bare name is `Ours`. TeamSpeak's `platform/teamspeak/inbound.rs:classify` reads only a leading `!`; `/status` there is ordinary conversation, and it has no other-bot case because a TS3 text message has no bot-to-bot restriction to reason about.

| `CommandOwner` | Meaning |
|---|---|
| `NotACommand` | not a slash message at all |
| `Theirs` | `/name@other_bot` — a command for another bot |
| `Ours { name, payload }` | our slash message, known name or not |

`/start` never reaches the table: it is the platform's own convention, declared in the platform's own list (`platform/telegram/inbound/command.rs:OWN_COMMANDS`, which the menu also reads) and answered before the table is consulted.

Ownership is judged on the `@` suffix, matched against the bot's own name as written — the same exactness a mention gets (`platform/telegram/inbound/arrival.rs:is_mentioning_user`) — and the payload is sliced at the first whitespace so the user's own spacing survives. `/foo@` lands in `Theirs`; `/foo` and `/Lang` land in `Ours` with the name's case intact, leaving `find` to reject them.

Recognition then splits `Ours` in two: a name the table knows goes to `agent/command/handle.rs:execute` with its `Invocation`; a name it does not know is an ordinary message, because a person typing an unknown slash is still talking to the agent.

### What enters the conversation

A message is stored only when it can form a complete exchange:

| Message | `message` table | Answer |
|---|---|---|
| our command, payload readable | no | host copy, or the agent inside the conversation |
| our command, payload unreadable | no | host copy naming the argument and the reason (`HostCopy::BadArgs`), no wake |
| `/name@other_bot` | no | dropped, with a debug log |
| a button we put out, pressed | no | the page redraws, or the confirmed work is enqueued |
| anything else | yes | the agent, woken by the usual reason |

A command for another bot is never stored because its other half never arrives: Telegram does not deliver messages from bots to bots, so the reply is unreachable by construction. A recognized command writes no message: an agent task joins the accumulation as a command entry carrying the `AgentCommand` itself, a host operation that changes a setting appends a `HostAction` entry for the next Turn (one that changes nothing appends nothing), and a confirmation is host copy until the page it is on is confirmed.

### Answering

`execute` takes a `CommandContext` — the origin Chat, its Conversation, the `ConversationSettings` facade, the `AgentRegistry` and the domain services — assembled by the shared inbound tail (`agent/inbound/mod.rs:Intake::run_command`), and a `Command`, and answers with a `CommandOutcome`: the `Reply` the host owes and, when the command changed a Conversation setting, the `HostAction` that records it. The reply is one of:

- `Reply::Said` carries a `HostCopy` and the language it must be spoken in; the host renders it through `agent/host_copy.rs:HostCopy::render`, and the platform delivers what it is handed.
- `Reply::Dialog` carries a `Dialog` — the `DialogArg` (which command's question this is, and how far it got), the language and the `DialogFallback` a channel without pages answers with: the answer is a question, so the host puts its pages out over the platform's buttons and reads the presses back. The page table is one engine for every command (`agent/command/dialog.rs`); `agent/command/purge/` is what its pages say. A page only collects arguments the line did not carry; it never owns an action.
- `Reply::Started` carries the work a complete command line named — a `PurgeSpec`, or a `DigestStart` with its stretch, delivery and report language. `agent/inbound/mod.rs:Intake::start` submits it through the same functions a confirmation press uses (`purge::queue`, `digest::start`), so nothing is reachable only through a button.
- `Reply::Silent` is an agent task: it joins the Conversation's accumulation as a command entry, and the answer arrives in the conversation.

A handler never appends the action itself: `execute` records it once, after the handler and before the caller sees the outcome, so the change is durable even when the host reply fails to send; a failed append is an error, never silence.

A `Reply::Dialog` on a channel whose `Intake` holds no choice surface is answered from the dialog's own `fallback` (`agent/command/mod.rs:DialogFallback`): `Missing(ArgName)` is a line that stopped at an argument and is answered exactly as a command line answers it — `HostCopy::BadArgs` names the argument (`RANGE`, or `--yes` for an unconfirmed purge) and appends the synopsis (`agent/channel.rs:ChannelAccess::command_prefix` supplies the platform's own prefix) — while `Answer(Said)` is a line that was already complete and only lacks the buttons, so the host speaks the reply it had composed. The bare content-policy line is the one `Answer`: it is complete without a preset, so a channel without pages reports the effective axes instead of naming a missing argument. The inbound tail consequently has no case of its own for that line — the dialog carries the answer it owes. The host never explains the platform's UI; a command that cannot finish says which argument is missing.

The language is resolved once per reply through `agent/settings.rs:ConversationSettings::language` — the same resolution the agent itself uses — and for `/lang` it is resolved *after* the preference is written, so setting a language is confirmed in the language it just became. `/status` renders the agent's own block (`agent/chat/handle.rs:ConversationAgentStatus`) in that one language.

### Host feedback

A host operation is answered by the host, but the agent still has to learn that the ground moved. `/persona`, `/model`, `/lang` and `/content-policy` return a `HostAction` (`domain/vo/host_action.rs:HostAction`) beside the reply: `PersonaChanged`, `ModelChanged`, `LanguageChanged` or `ContentPolicyChanged`. The value names the setting, never its replacement — the next Turn reads the value from the Conversation's settings snapshot — so the accumulation carries no second copy of mutable state.

- **A set or a reset emits one**, from each setting handler in `agent/command/handle.rs` (`CommandOutcome::with_action`).
- **A query, an unsupported value or a refused write emits none** — `Current`, `LanguageArg::Unsupported`, an unknown model alias, an empty or over-long persona, an unreadable payload. Something was said or refused; nothing changed, so nothing is recorded.
- **A stricter content policy emits both** the `ContentPolicyChanged` host action and the `PolicyChanged` entry that reopens the chapter: the next Turn consumes the first, the wrap-up consumes the second.
- **A command that changes no Conversation setting emits none** — `/status`, `/start`, `/purge` and `/digest` report, confirm or start a pass instead.
- **The entry is silent.** Its audience is `EntryAudience::Context`, it answers no `WakeReason`, and it moves no scheduler clock, so nothing wakes for it. The next normal Turn of the Conversation reads it as `<host_actions>` in the context XML and consumes it with the rest of its batch; a batch that holds nothing but host actions reaches no Turn at all.

The agent therefore hears about each change exactly once, in the Turn it would have run anyway, and never as an instruction.

### Arguments

`RANGE` is the one stretch spelling shared by `/purge` and `/digest`: `all`, a whole number of hours or days back from now (`24h`, `7d`), or two local dates covering whole days with the last one included (`2026-10-01..2026-10-07`).

- `/status` — the agent's status block; when the agent is not running the host copy says so.
- `/lang [CODE] [--reset]` — bare reports the current language; `reset` (the word or the flag) deletes the per-Conversation value so the configured default applies again; any other value is parsed case-insensitively as `Language`, so `/lang JA` works. An unknown code is answered as such rather than as a generic unreadable payload. A code and `--reset` together are refused. Any whitelisted member may set it, and it is per Conversation, never per user.
- `/model [ALIAS] [--reset]` — bare reports the effective alias/model and the configured aliases; `ALIAS` writes the Conversation's model preference, and `reset` (the word or the flag) clears it. An unknown alias writes nothing. Model changes never reopen the chapter.
- `/contentpolicy [PRESET] [--reset]` — `PRESET` is the typed `config/conversation/content_policy.rs:ContentPolicyPreset`: `safe`, `standard`, `mature` or `unrestricted`; any other token is the parser's `ArgError::BadValue` on `PRESET`, never a silently ignored argument. Bare opens a picker over those presets and reset on a channel with pages, and reports the effective axes where there are none; `--reset` restores `[conversation.content-policy]`. `unrestricted` adds no application-side restriction (provider/platform blocks remain). A stricter change queues `PolicyChanged` and reopens through the ordinary wrap-up; an upgrade does not. The canonical spelling is `content-policy`, also accepted when typed; Telegram's menu uses `contentpolicy` because its command vocabulary has no hyphen, and `--help` echoes the spelling that invoked it. With `allow-content-policy-change = false` the command is absent from the menu, help and parser, so a typed line reads as ordinary conversation.
- `/persona [TEXT...] [--reset]` — bare reports the effective persona (`none` when absent); `--reset` clears it. Text over 4096 characters or empty is refused. With `allow-persona-change = false` the command is absent from the menu, help and parser. The persona never reopens the chapter.
- `/explain [--] [TEXT...]`, alias `/explan` — enqueue an `explain` entry carrying the target (empty when none was named); without a target the agent explains the object most recently mentioned and not yet explained. The first free token starts the text; `--` is how text that begins with a dash is written.
- `/digest [RANGE] [--lang CODE] [--to here|me]` — write a report over a stretch of this chat's own record. `RANGE` must be 1–365 whole days (a date range counts its inclusive days); `--lang` names the language of this one report (any `Language`, read case-insensitively), and `--to` names where it lands — `here` (the chat, the default) or `me` (the sender's private chat; in a private chat the two are the same). Nothing is enqueued and no chat data is written: the pass reads only, runs in the Conversation's Busy slot, and delivers its report as a `.md` file with the window as its caption. A bare `/digest` asks for the range on a page; a `--to` already named is kept, so the pass starts as soon as the range is chosen. A named range is complete and runs without a page. The page is taken away once the document is there, and a refused removal is only logged; a private delivery first sends one queued line, and a delivery the platform refuses keeps the report on the page with a retry and the chat itself as a way out. A complete line whose delivery is refused is told so, and the running notice stays. On TeamSpeak a report in a room or the server chat is uploaded to that channel's file repository and a path line names it; `--to me` arrives as private text chunks instead, because a channel repository is shared.
- `/purge [RANGE] [--surfaces LIST] [--yes]` — wipe this Conversation's own traces over a stretch. `--surfaces` is a comma-separated subset of `agent`, `files`, `telegram` and `derived`, all four when none is named; `telegram` is the wire name for the platform-message surface on every platform. `--yes` is the confirmation in command form: `/purge 7d --yes` runs, `/purge 7d` asks on a page and is answered with the full form on a channel without one; a bare `/purge` starts at the page that offers the two questions. Surfaces without a range are kept for the page. A confirmation page and `--yes` submit the same `PurgeSpec` to `agent/command/purge/mod.rs:queue`, and nothing is enqueued until one of them does. The page itself carries seven range presets and is bound to whoever opened it.

The command itself is `domain/vo/command.rs:AgentCommand` — a domain value, because a `Command` entry stores it; the instruction the agent reads for one lives in `agent/events/wake.rs:AgentCommand::instruction`, in the Conversation's language.

### Host copy and the menu

Host copy lives in `agent/host_copy.rs`: `HostCopy` says what the host says — the language, model, content-policy and persona replies are their own variants — and `HostCopy::render` says it in the chat's built-in set; a dialog's pages and a purge's receipt live beside their command (`<command>/copy.rs`). `--help` renders the command's description, its synopsis, and the complete examples the declaration carries; examples are payloads rendered behind the platform's prefix, and a page-only form is never an example. The platform only delivers what it is handed — MarkdownV2 and the plain-text fallback live in `platform/telegram/outbound/service.rs:send_markdown_with_fallback`, while TeamSpeak sends the text verbatim and chunks it at 1024 UTF-8 bytes — behind `agent/channel.rs:ChannelAccess::notice`, which the inbound tail's host-copy path calls. `HostCopy::Unauthorized` is produced by the whitelist path and never by a command, and `HostCopy::Greeting` by `/start`, which is Telegram's own convention rather than an agent table row: `platform/telegram/inbound/arrival.rs` reads it off a message that names it, before the table is consulted (a deep link's payload is ignored), and its menu copy is declared with it in `platform/telegram/inbound/command.rs:OWN_COMMANDS`. TeamSpeak has no greeting convention, so its arrivals never produce `Greeting`.

The menu is the platform's own declarations first (`platform/telegram/inbound/command.rs:OWN_COMMANDS`) and then the agent's `agent/command/spec.rs:DECLARATIONS` in declaration order (`platform/telegram/ui/menu.rs:bot_commands`). It lists every command: the menu is an index of what a chat can run from a line, and a page is only an extra way to collect the same arguments. Telegram takes a bare name — its `BotCommand.command` allows only lowercase letters, digits and underscores — and renders the slash itself. Scopes are derived, not listed: `platform/telegram/ui/menu.rs:scopes` pushes the English catalog as the scope without a `language_code` — the fallback a client whose language has no scope of its own reads — plus one `language_code` scope per client code reading another catalog, because Telegram hands a client the most specific scope it matches; the code is a two-letter ISO 639-1 one, the only form Telegram accepts.

## Boundaries

- No platform type reaches `agent/command/`: the wire form and the rendering stay at the platform edge, and the semantics take a `CommandContext`.
- Whitelisting precedes all of it (`agent/inbound/mod.rs:Intake::accept` asks `agent/channel.rs:ChannelAccess::admits`); a chat outside the allowlist never reaches the command table, and its `/start` is the one thing the platform answers there.
- A command is not a tool: a host operation never wakes the agent — a setting change reaches it later, as context — and an agent task owes the host no copy.

## Trade-offs and Reasons

- **Ownership is judged before recognition** — the `@` suffix is the user's own statement of who the message is for, so it decides even for names the table knows; Telegram's own delivery rules agree (`docs/topics/platform.md`).
- **A recognized command writes no message** — its accumulation entry is the single representation; storing the call as a message as well would put the same act in the conversation twice.
- **An unreadable payload is answered, not handed on** — the user addressed our command, so the host says it could not read it; passing it to the agent would produce a second answer to the same message.
- **Exact names** — matching the table literally keeps the contract independent of the platform; case-insensitivity would have to be re-decided per platform.
- **The command line is the contract, the page is a UI** — every action is reachable from one line, so a platform without buttons loses only the picker; the price is an explicit `--yes` for a destructive run and a longer form where a button would be shorter.
- **A host change is recorded, not announced** — the host's own reply is for the person and the `HostAction` is for the next Turn; waking the agent to tell it something it already missed would spend a Turn on the past, while leaving no entry would let it answer in a setting the Conversation no longer runs with.
- **One grammar spells the synopsis** — the parser and the form a person is shown cannot drift; the price is that a new value kind needs its parser beside the declaration, not inline in a command.

## Pitfalls / common mistakes

- **A new command needs its whole row**: `name`, `aliases`, `copy`, `grammar`, `examples`, a `parse` function, and the arm that answers it — `handle.rs:answer` for a system command, an `AgentCommand` variant and its wake instruction for an agent task. The matches are exhaustive, so a missing arm is a compile error. The synopsis is derived from the grammar, and a test pins it; there is no second spelling table to keep in step.
- **Every command must have a complete line** — `handle.rs:submit_purge` and `handle.rs:submit_digest` decide from the arguments alone and name the argument still missing; a complete one becomes `Reply::Started`. A page is only reached when an argument is missing. If a command adds a page, its confirmation must call the same submission function (`purge::queue`, `digest::start`), never a parallel implementation, and `spec.rs`'s complete-line test is where the property is pinned.
- **`@bot` matching is exact** — `classify` compares the suffix with the bot's own name as written and lowercases nothing else; only `/lang` normalizes its own argument.
- **`HostCopy::Status(None)` is the host's own copy**: `/status` answers even when no agent exists for the chat.
- **A setting command returns its action with the reply** — `CommandOutcome::with_action`; the handler never appends the entry itself, and `Intake` records it before the reply leaves. A query path that returned an action would tell the next Turn a value moved when it did not, and a set that forgot one leaves the agent answering in the old setting.
- **A menu name carries no slash** — Telegram rejects or mis-renders one, and the client adds it.
- **A payload sliced from `split_whitespace` would lose the user's spacing** — `classify` slices it out of the original text instead.
- **A raw command's text starts at its first free token** — `explain --verbose` is an unknown option; `explain -- --verbose` is the text. This is the standard `--` rule, and it is the only way to explain something that begins with a dash.
- **A command that answers with a question owns three more seams** — `ChannelChoices::ask`/`edit_choice`/`answer_callback` on the platform, an `ArrivalText::Choice` arm in the arrival, and the pages the engine holds (`agent/command/dialog.rs`) with each command's own page table (`agent/command/purge/dialog.rs`, `agent/command/digest/dialog.rs`). The engine, the binding to chat and sender, the one-press tokens and the expiry are shared; a second dialog also needs its `SystemCommand`/`DialogArg` pair, an `answer` arm, an open arm and a `resolve_choice` arm in the inbound tail. What a press means is settled in the agent layer before anything is enqueued; the platform only carries tokens back. A platform without that surface implements only `ChannelAccess` and is given no page table at all.

## Evolution Direction

- Another platform takes the declaration table as it stands and owns only the classification and the menu (where it has one), the way `platform/telegram/` and `platform/teamspeak/` do: the orchestration above them is `agent/inbound/mod.rs:Intake`, shared by every platform (`docs/architecture.md`).
