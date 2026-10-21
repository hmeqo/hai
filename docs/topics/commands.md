# Commands

> The command table, who a slash message belongs to, and the menu. Must-read when adding a command or changing what one replies.

## Overview

A command is one row of `agent/command/spec.rs:DECLARATIONS` — its name and its aliases, its menu copy (`agent/command/menu.rs`), the positionals and options it accepts (`agent/command/args.rs`), and the parse that turns them into a `Command` (`agent/command/parse.rs`). The same row spells its synopsis, so the form a person is shown and the form the parser reads cannot drift apart. Nothing else holds that list: the menu, the argument parsing and the answering all read the row.

**The command line is the contract; a page is a UI.** Every command has one line that names everything it needs and reaches the action without a page. A platform without buttons loses the page, never the command: an unfinished line is answered with the full form. Telegram owns the adapter — deciding whose message a slash line is, rendering a menu, sending a reply (`platform/telegram/`).

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
- **A refusal names the argument and the reason** (`ArgError`: unknown option, missing or unexpected value, duplicate, conflict, extra or missing positional, unreadable value, out of range), and `agent/host_copy.rs:HostCopy::BadArgs` renders it in the chat's language and appends the synopsis, so the form is repeated wherever the line failed.
- **A flag that appeared is read by presence, never by emptiness** — `--reset` holds nothing and is still given. A command's own cross-checks (an argument that cannot come with another) run after the payload is read.

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

`execute` takes a `CommandContext` — the chat, the `Settings` facade, the `AgentRegistry` and the domain services — assembled by the shared inbound tail (`agent/inbound/mod.rs:Intake::run_command`), and a `Command`, and answers with a `Reply`:

- `Reply::Said` carries a `HostCopy` and the language it must be spoken in; the host renders it through `agent/host_copy.rs:HostCopy::render`, and the platform delivers what it is handed.
- `Reply::Dialog` carries a `DialogArg` (which command's question this is, and how far it got) and the language: the answer is a question, so the host puts its pages out over the platform's buttons and reads the presses back. The page table is one engine for every command (`agent/command/dialog.rs`); `agent/command/purge/` is what its pages say. A page only collects arguments the line did not carry; it never owns an action.
- `Reply::Started` carries the work a complete command line named — a `PurgeSpec`, or a `DigestStart` with its stretch, delivery and report language. `agent/inbound/mod.rs:Intake::start` submits it through the same functions a confirmation press uses (`purge::queue`, `digest::start`), so nothing is reachable only through a button.
- `Reply::Silent` is an agent task: it joins the chat's accumulation as a command entry, and the answer arrives in the conversation.

A `Reply::Dialog` on a channel whose `ChannelCapabilities::choices` is false is answered the way a command line answers it: `HostCopy::BadArgs` names the argument the line stopped at — `RANGE`, or `--yes` for an unconfirmed purge — and appends the synopsis (`agent/channel.rs:ChannelAccess::command_prefix` supplies the platform's own prefix). The host never explains the platform's UI; a command that cannot finish says which argument is missing.

The language is resolved once per reply through `agent/settings.rs:Settings::language` — the same resolution the agent itself uses — and for `/lang` it is resolved *after* the preference is written, so setting a language is confirmed in the language it just became. `/status` renders the agent's own block (`agent/chat/handle.rs:ChatAgentStatus`) in that one language.

### Arguments

`RANGE` is the one stretch spelling shared by `/purge` and `/digest`: `all`, a whole number of hours or days back from now (`24h`, `7d`), or two local dates covering whole days with the last one included (`2026-10-01..2026-10-07`).

- `/status` — the agent's status block; when the agent is not running the host copy says so.
- `/lang [CODE] [--reset]` — bare reports the current language; `reset` (the word or the flag) deletes the per-chat row so the configured default applies again; any other value is parsed case-insensitively as `Language`, so `/lang JA` works. An unknown code is answered as such rather than as a generic unreadable payload. A code and `--reset` together are refused. Any whitelisted member may set it, and it is per chat, never per user.
- `/organizememory` — enqueue an `organize_memory` entry.
- `/explain [--] [TEXT...]`, alias `/explan` — enqueue an `explain` entry carrying the target (empty when none was named); without a target the agent explains the object most recently mentioned and not yet explained. The first free token starts the text; `--` is how text that begins with a dash is written.
- `/digest [RANGE] [--lang CODE] [--to here|me]` — write a report over a stretch of this chat's own record. `RANGE` must be 1–365 whole days (a date range counts its inclusive days); `--lang` names the language of this one report (any `Language`, read case-insensitively), and `--to` names where it lands — `here` (the chat, the default) or `me` (the sender's private chat). Nothing is enqueued and no chat data is written: the pass reads only, runs in the chat's Busy slot, and delivers its report as a `.md` file with the window as its caption. A bare `/digest` asks for the range on a page; a `--to` already named is kept, so the pass starts as soon as the range is chosen. A named range is complete and runs without a page. The page is taken away once the document is there, and a refused removal is only logged; a private delivery first sends one queued line, and a delivery the platform refuses keeps the report on the page with a retry and the chat itself as a way out. A complete line whose delivery is refused is told so, and the running notice stays.
- `/purge [RANGE] [--surfaces LIST] [--yes]` — wipe this chat's own traces over a stretch. `--surfaces` is a comma-separated subset of `agent`, `files`, `telegram` and `derived`, all four when none is named. `--yes` is the confirmation in command form: `/purge 7d --yes` runs, `/purge 7d` asks on a page and is answered with the full form on a channel without one; a bare `/purge` starts at the page that offers the two questions. Surfaces without a range are kept for the page. A confirmation page and `--yes` submit the same `PurgeSpec` to `agent/command/purge/mod.rs:queue`, and nothing is enqueued until one of them does. The page itself carries seven range presets and is bound to whoever opened it.

The command itself is `domain/vo/command.rs:AgentCommand` — a domain value, because a `Command` entry stores it; the instruction the agent reads for one lives in `agent/events/wake.rs:AgentCommand::instruction`, in the chat's language.

### Host copy and the menu

Host copy lives in `agent/host_copy.rs`: `HostCopy` says what the host says — the language replies are their own variants — and `HostCopy::render` says it in the chat's built-in set; a dialog's pages and a purge's receipt live beside their command (`<command>/copy.rs`). `--help` renders the command's description, its synopsis, and the complete examples the declaration carries; examples are payloads rendered behind the platform's prefix, and a page-only form is never an example. The platform only delivers what it is handed — MarkdownV2 and the plain-text fallback live in `platform/telegram/outbound/service.rs:send_markdown_with_fallback`, behind `agent/channel.rs:ChannelAccess::notice`, which the inbound tail's host-copy path calls. `HostCopy::Unauthorized` is produced by the whitelist path and never by a command, and `HostCopy::Greeting` by `/start`, which is the platform's own convention rather than an agent table row: `platform/telegram/inbound/arrival.rs` reads it off a message that names it, before the table is consulted (a deep link's payload is ignored), and its menu copy is declared with it in `platform/telegram/inbound/command.rs:OWN_COMMANDS`.

The menu is the platform's own declarations first (`platform/telegram/inbound/command.rs:OWN_COMMANDS`) and then the agent's `agent/command/spec.rs:DECLARATIONS` in declaration order (`platform/telegram/ui/menu.rs:bot_commands`). It lists every command: the menu is an index of what a chat can run from a line, and a page is only an extra way to collect the same arguments. Telegram takes a bare name — its `BotCommand.command` allows only lowercase letters, digits and underscores — and renders the slash itself. Scopes are derived, not listed: `platform/telegram/ui/menu.rs:scopes` pushes the English catalog as the scope without a `language_code` — the fallback a client whose language has no scope of its own reads — plus one `language_code` scope per client code reading another catalog, because Telegram hands a client the most specific scope it matches; the code is a two-letter ISO 639-1 one, the only form Telegram accepts.

## Boundaries

- No platform type reaches `agent/command/`: the wire form and the rendering stay at the platform edge, and the semantics take a `CommandContext`.
- Whitelisting precedes all of it (`agent/inbound/mod.rs:Intake::accept` asks `agent/channel.rs:ChannelAccess::admits`); a chat outside the allowlist never reaches the command table, and its `/start` is the one thing the platform answers there.
- A command is not a tool: a host operation never wakes the agent, and an agent task owes the host no copy.

## Trade-offs and Reasons

- **Ownership is judged before recognition** — the `@` suffix is the user's own statement of who the message is for, so it decides even for names the table knows; Telegram's own delivery rules agree (`docs/topics/platform.md`).
- **A recognized command writes no message** — its accumulation entry is the single representation; storing the call as a message as well would put the same act in the conversation twice.
- **An unreadable payload is answered, not handed on** — the user addressed our command, so the host says it could not read it; passing it to the agent would produce a second answer to the same message.
- **Exact names** — matching the table literally keeps the contract independent of the platform; case-insensitivity would have to be re-decided per platform.
- **The command line is the contract, the page is a UI** — every action is reachable from one line, so a platform without buttons loses only the picker; the price is an explicit `--yes` for a destructive run and a longer form where a button would be shorter.
- **One grammar spells the synopsis** — the parser and the form a person is shown cannot drift; the price is that a new value kind needs its parser beside the declaration, not inline in a command.

## Pitfalls / common mistakes

- **A new command needs its whole row**: `name`, `aliases`, `copy`, `grammar`, `examples`, a `parse` function, and the arm that answers it — `handle.rs:answer` for a system command, an `AgentCommand` variant and its wake instruction for an agent task. The matches are exhaustive, so a missing arm is a compile error. The synopsis is derived from the grammar, and a test pins it; there is no second spelling table to keep in step.
- **Every command must have a complete line** — `handle.rs:submit_purge` and `handle.rs:submit_digest` decide from the arguments alone and name the argument still missing; a complete one becomes `Reply::Started`. A page is only reached when an argument is missing. If a command adds a page, its confirmation must call the same submission function (`purge::queue`, `digest::start`), never a parallel implementation, and `spec.rs`'s complete-line test is where the property is pinned.
- **`@bot` matching is exact** — `classify` compares the suffix with the bot's own name as written and lowercases nothing else; only `/lang` normalizes its own argument.
- **`HostCopy::Status(None)` is the host's own copy**: `/status` answers even when no agent exists for the chat.
- **A menu name carries no slash** — Telegram rejects or mis-renders one, and the client adds it.
- **A payload sliced from `split_whitespace` would lose the user's spacing** — `classify` slices it out of the original text instead.
- **A raw command's text starts at its first free token** — `explain --verbose` is an unknown option; `explain -- --verbose` is the text. This is the standard `--` rule, and it is the only way to explain something that begins with a dash.
- **A command that answers with a question owns three more seams** — `ChannelAccess::ask`/`edit_choice`/`answer_callback` on the platform, an `ArrivalText::Choice` arm in the arrival, and the pages the engine holds (`agent/command/dialog.rs`) with each command's own page table (`agent/command/purge/dialog.rs`, `agent/command/digest/dialog.rs`). The engine, the binding to chat and sender, the one-press tokens and the expiry are shared; a second dialog also needs its `SystemCommand`/`DialogArg` pair, an `answer` arm, an open arm and a `resolve_choice` arm in the inbound tail. What a press means is settled in the agent layer before anything is enqueued; the platform only carries tokens back.

## Evolution Direction

- A second platform takes the declaration table as it stands and owns only the classification and the menu, the way `platform/telegram/inbound/command.rs` does: the orchestration above them is `agent/inbound/mod.rs:Intake`, shared by every platform (`docs/architecture.md`).
