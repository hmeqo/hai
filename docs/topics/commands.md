# Commands

> The command table, who a slash message belongs to, and the menu. Must-read when adding a command or changing what one replies.

## Overview

A command is one row of `agent/command/spec.rs:DECLARATIONS` — its name and its aliases, its menu copy (`agent/command/menu.rs`), the parse that reads its payload (`agent/command/flags.rs`, `agent/command/parse.rs`), and the `Interaction` it needs from a platform. Nothing else holds that list: the menu, the argument parsing and the answering all read the row. Telegram owns the adapter: deciding whose message a slash line is, rendering a menu, sending a reply (`platform/telegram/`).

## Design

### The declaration

Each identity answers these questions:

- `name` and `aliases` — the wire spelling. Names match **exactly**: the command contract does not inherit a platform's spelling habits, so `/explan` is `Explain` and `/Lang` is an ordinary message.
- `copy` — the menu copy, one entry per client catalog.
- `interaction` — what answering it takes from the platform: a plain reply, or a page of buttons.
- `parse(payload) -> Invocation` — the invocation the payload denotes, and the only place a `Command` is built. `Invocation::Ready` carries the command itself, typed: an `AgentCommand` for an agent task, a `SystemCommand` for a host operation (`LanguageArg` lives there); `Invocation::Unreadable(ArgError)` carries why the payload does not fit the command, and the host repeats that reason to the person.

`find` resolves a name or an alias to a row. `/digest`'s own grammar holds its day count inside 1–365 and refuses anything else with the reason (`agent/command/parse.rs`); a count nobody named is the dialog's question, not a default.

### The payload grammar

Every payload is read through one tokenizer (`agent/command/args.rs`), and a command declares only the shape it takes — `Grammar::None` (nothing), `Grammar::Flags` (a set of flags, each spelled once by a `FlagName` enum) or `Grammar::RawText` (the payload verbatim, `/explain`'s case):

- **A bare word and the flag naming it are one argument.** `/purge all agent,files` is `/purge --range all --surfaces agent,files`; a bare token fills the first positional flag the grammar declares that is still unset.
- **A value is written `--name value` or `--name=value`**, and the long kebab-case name is the only spelling.
- **Anything starting with `-` is a flag**, so a token it does not know is refused by name rather than taken as a bare word; `/explain` is the one command that reads its payload verbatim, dashes and all.
- **A refusal names the argument and the reason** (`ArgError`: unknown, missing value, unexpected value, given twice, conflicting, extra positional, unreadable value, out of range), and `agent/host_copy.rs:HostCopy::BadArgs` renders it in the chat's language.
- **A flag that appeared is read by presence, never by emptiness** — `--reset` holds nothing and is still given. A command's own cross-checks (a flag that cannot come with another) run after the payload is read.

### Who a message belongs to

`platform/telegram/inbound/command.rs:classify` answers **ownership**, independently of recognition:

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

A command for another bot is never stored because its other half never arrives: Telegram does not deliver messages from bots to bots, so the reply is unreachable by construction. A recognized command writes no message: an agent task joins the accumulation as a command entry carrying the `AgentCommand` itself, a host operation leaves no trace because the host answered it on the spot, and a confirmation is host copy until the page it is on is confirmed.

### Answering

`execute` takes a `CommandContext` — the chat, the `Settings` facade, and the `AgentRegistry` — assembled by the shared inbound tail (`agent/inbound/mod.rs:Intake::run_command`), and answers with a `Reply`:

- `Reply::Said` carries a `HostCopy` and the language it must be spoken in; the host renders it through `agent/host_copy.rs:HostCopy::render`, and the platform delivers what it is handed.
- `Reply::Dialog` carries a `DialogArg` (which command's question this is, and how far it got) and the language: the answer is a question, so the host puts its pages out over the platform's buttons and reads the presses back. The page table is one engine for every command (`agent/command/dialog.rs`); `agent/command/purge/` is what its pages say. Nothing is enqueued until the confirmation page is pressed.
- `Reply::Silent` is an agent task: it joins the chat's accumulation as a command entry, and the answer arrives in the conversation.

The language is resolved once per reply through `agent/settings.rs:Settings::language` — the same resolution the agent itself uses — and for `/lang` it is resolved *after* the preference is written, so setting a language is confirmed in the language it just became. `/status` renders the agent's own block (`agent/chat/handle.rs:ChatAgentStatus`) in that one language.

### Arguments

- `/status` — the agent's status block; when the agent is not running the host copy says so.
- `/lang [code|reset]` (`--code <code>` / `--reset`) — bare reports the current language, `reset` deletes the per-chat row so the configured default applies again, and any other value is parsed case-insensitively as `Language`, so `/lang JA` works; an unknown code is answered as such rather than as a generic unreadable payload. Any whitelisted member may set it, and it is per chat, never per user.
- `/organizememory` — enqueue an `organize_memory` entry.
- `/explain [target]`, alias `/explan` — enqueue an `explain` entry carrying the target (empty when none was named); without a target the agent explains the object most recently mentioned and not yet explained.
- `/digest [N]` (`--days N`, `--lang <code>`) — write a report over a stretch of this chat's own record: a count of days back from now (1–365), or one of the three the page offers. `--lang` names the language of this one report (any `Language`, read case-insensitively), and the confirmation says so when it differs from the chat's; a bare word fills the first flag still unset, in declaration order, so `/digest 7 ja` names both, and `--lang ja` names only the language. Nothing is enqueued and no chat data is written — the pass reads only, runs in the chat's Busy slot, and delivers its report as a `.md` file with the window as its caption, to the chat itself or, in a group, to the presser's private chat; the confirmation page is where that destination is chosen, before the pass runs. Anybody in the chat may press the page (in a group the private-chat choice needs a presser the platform named). The page is taken away once the document is there, and a refused removal is only logged; a private delivery first sends one queued line, and a delivery the platform refuses keeps the report on the page with a retry and the chat itself as a way out. A count outside 1–365 is refused with its reason.
- `/purge [range] [surfaces]` — ask to wipe this chat's own traces over a stretch. Bare `/purge` starts at a first page offering the two questions (`时间范围`/`Time range` and `清理内容`/`What to purge`); the range page carries seven presets — the last five minutes, thirty minutes, an hour, a day, a week, thirty days, and all time — and a named range (`all`, `<n>h`, `<n>d`, or `YYYY-MM-DD..YYYY-MM-DD` for whole local days, the last one included) goes straight to the confirmation. Surfaces are a comma-separated subset of `agent`, `files`, `telegram` and `derived`, all four when none is named (`/purge all agent,files`); a bare word fills the range slot first, so `/purge agent` is refused, while `--surfaces agent` names them deliberately and goes on to the range page with that choice kept. Nothing is enqueued until the person confirms; the dialog is bound to whoever opened it.

The command itself is `domain/vo/command.rs:AgentCommand` — a domain value, because a `Command` entry stores it; the instruction the agent reads for one lives in `agent/events/wake.rs:AgentCommand::instruction`, in the chat's language.

### Host copy and the menu

Host copy lives in `agent/host_copy.rs`: `HostCopy` says what the host says — the language replies are their own variants — and `HostCopy::render` says it in the chat's built-in set; a dialog's pages and a purge's receipt live beside their command (`<command>/copy.rs`). The platform only delivers what it is handed — MarkdownV2 and the plain-text fallback live in `platform/telegram/outbound/service.rs:send_markdown_with_fallback`, behind `agent/channel.rs:ChannelAccess::notice`, which the inbound tail's host-copy path calls. `HostCopy::Unauthorized` is produced by the whitelist path and never by a command, and `HostCopy::Greeting` by `/start`, which is the platform's own convention rather than an agent table row: `platform/telegram/inbound/arrival.rs` reads it off a message that names it, before the table is consulted (a deep link's payload is ignored), and its menu copy is declared with it in `platform/telegram/inbound/command.rs:OWN_COMMANDS`.

The menu is the platform's own declarations first (`platform/telegram/inbound/command.rs:OWN_COMMANDS`) and then the agent's `agent/command/spec.rs:DECLARATIONS` in declaration order, filtered through `Interaction::supported_by` (`platform/telegram/ui/menu.rs:bot_commands`): a command that needs a page of buttons is not offered on a channel whose capabilities say it cannot put one out, and typing it there is answered with `HostCopy::Unsupported` instead of a page. Telegram takes a bare name — its `BotCommand.command` allows only lowercase letters, digits and underscores — and renders the slash itself. Scopes are derived, not listed: `platform/telegram/ui/menu.rs:scopes` pushes the English catalog as the scope without a `language_code` — the fallback a client whose language has no scope of its own reads — plus one `language_code` scope per client code reading another catalog, because Telegram hands a client the most specific scope it matches; the code is a two-letter ISO 639-1 one, the only form Telegram accepts.

## Boundaries

- No platform type reaches `agent/command/`: the wire form and the rendering stay at the platform edge, and the semantics take a `CommandContext`.
- Whitelisting precedes all of it (`agent/inbound/mod.rs:Intake::accept` asks `agent/channel.rs:ChannelAccess::admits`); a chat outside the allowlist never reaches the command table, and its `/start` is the one thing the platform answers there.
- A command is not a tool: a host operation never wakes the agent, and an agent task owes the host no copy.

## Trade-offs and Reasons

- **Ownership is judged before recognition** — the `@` suffix is the user's own statement of who the message is for, so it decides even for names the table knows; Telegram's own delivery rules agree (`docs/topics/platform.md`).
- **A recognized command writes no message** — its accumulation entry is the single representation; storing the call as a message as well would put the same act in the conversation twice.
- **An unreadable payload is answered, not handed on** — the user addressed our command, so the host says it could not read it; passing it to the agent would produce a second answer to the same message.
- **Exact names** — matching the table literally keeps the contract independent of the platform; case-insensitivity would have to be re-decided per platform.

## Pitfalls / common mistakes

- **A new command needs its whole row**: `name`, `aliases`, `copy`, `interaction`, a `parse` function, and the arm that answers it — `handle.rs:answer` for a system command, an `AgentCommand` variant and its wake instruction for an agent task. The matches are exhaustive, so a missing arm is a compile error. A command that reads flags also declares its `FlagName` enum and grammar — the spellings live there once, and a consistency test pins each enum to its grammar.
- **`@bot` matching is exact** — `classify` compares the suffix with the bot's own name as written and lowercases nothing else; only `/lang` normalizes its own argument.
- **`HostCopy::Status(None)` is the host's own copy**: `/status` answers even when no agent exists for the chat.
- **A menu name carries no slash** — Telegram rejects or mis-renders one, and the client adds it.
- **A payload sliced from `split_whitespace` would lose the user's spacing** — `classify` slices it out of the original text instead.
- **A command that answers with a question owns three more seams** — `ChannelAccess::ask`/`edit_choice`/`answer_callback` on the platform, an `ArrivalText::Choice` arm in the arrival, and the pages the engine holds (`agent/command/dialog.rs`) with each command's own page table (`agent/command/purge/dialog.rs`, `agent/command/digest/dialog.rs`). The engine, the binding to chat and sender, the one-press tokens and the expiry are shared; a second dialog also needs its `SystemCommand`/`DialogArg` pair, an `answer` arm, an open arm and a `resolve_choice` arm in the inbound tail, and a declaration row whose `interaction` is `Choices`. What a press means is settled in the agent layer before anything is enqueued; the platform only carries tokens back.

## Evolution Direction

- A second platform takes the declaration table as it stands and owns only the classification and the menu, the way `platform/telegram/inbound/command.rs` does: the orchestration above them is `agent/inbound/mod.rs:Intake`, shared by every platform (`docs/architecture.md`).
