# HAI

## Project overview

HAI is a long-term companion AI assistant running as a Telegram bot. Rust workspace (`hai` + `hai-macros`); layering `app → platform → agent → agentcore + config + domain + util/error/infra`. Spec authority: `docs/` (module-split); term authority: `docs/CONTEXT.md`; dependency rules and known violations: `docs/architecture.md`.

Key terminology boundaries (see `docs/CONTEXT.md`):
- **Turn ≠ Step**: a Turn is a React-loop execution unit (atomic); a Step is one LLM call
- **Chapter wrap-up ≠ summary**: wrap-up is the chapter-end retention summary; "summary" is a normal conversational reply
- **need-close ≠ DB state**: topic archive hint is rendering semantics (XML attribute); the DB has no automatic marker and no reopen capability
- **WakeEvent ≠ message**: pure notification (reason only); message content is fetched via `gather_messages(since_id)`
- **Session state = Idle/Busy** (not Idle/Active/Compacting)

## Commands

### Toolchain

- **Build**: Cargo workspace (`Cargo.toml`, members = `crates/*`, resolver 3) — `cargo build` (single crate: `cargo build --manifest-path crates/hai/Cargo.toml`)
- **Static check**: `cargo clippy --all-targets` — must be warning-free before delivery (currently only an external-dependency `proc-macro-error2` future-incompat note, not this project's issue)
- **Formatting**: `rustfmt.toml` (`imports_granularity = "Crate"`, `group_imports = "StdExternalCrate"`, `reorder_imports = true`); default rustfmt is fine
- **Database migration**: `hai db migrate` (`cli.rs:Commands::Db` → `domain/db.rs:run_migrations` runs `migrations/schema.sql`: idempotent table creation + embedding column + query index/idempotency-key unique constraints) — **migration is not auto-applied**; run manually after deploy; the IVFFlat vector index is built only by `db rebuild embeddings` (l2_ops)

### Project-specific commands

| Command | Purpose |
|---|---|
| `cargo run --bin hai` | no subcommand → `App::serve` (start bot) |
| `hai config` | print the loaded config (json/toml) |
| `hai db migrate` | create tables/columns/query indexes/unique constraints (vector index lifecycle: docs/topics/domain.md) |
| `hai log` | event log TUI (three panels); `--id N` prints a single event detail |
| `hai kb import/list/search/delete/reindex` | knowledge base management (idempotent import / semantic search / rebuild) |
| `hai db rebuild embeddings` | rebuild embedding vectors |

## Code style

<CRITICAL>
- **Terms** must follow `docs/CONTEXT.md` (Chat/Account/Topic/Memory/Turn/Step/chapter wrap-up/Session etc.); no invented names
- **Layering is inviolable**: `agentcore/` must not depend on `agent/` or `domain/`. Two known violations (`domain/service` → `agent::multimodal`; `agentcore → config` one-way — the original cycle is broken) are recorded in `docs/architecture.md`; **no new such dependencies**
- **Type-driven**: domain identifiers use newtype / enum (see `domain/vo/id.rs` `id_type!` and the strum enum); no bare `String` across boundaries, no new magic strings
- **Contract-failure**: missing data = violate and fail fast; no defensive fallbacks. The domain services' "swallow-error and return `Ok(None)`" is a known legacy; new code must not spread it
- **Explicit > Magic**: events go through `AgentEventPayload` (typed tagged enum, `domain/vo/event.rs`); no new string-based events
- **WakeEvent is a pure notification**: carries no message content and no DB message_id
- **React-loop tool execution is sequential** (await one by one), not parallel
- **Read the relevant `docs/` section** before touching session/communication/memory/config code
- **Same-turn doc update**: every code change updates the corresponding docs in the same turn; on conflict the code wins and the docs are corrected immediately ("code changed verbally, docs unchanged" = a delivery defect)
- **Anchors are always `file:symbol`**, never line numbers (they go stale)
- **Comment discipline**: no explanatory comments — code self-describes first (good naming / extracted functions convey intent); first consider refactoring so comments become unnecessary; needing an explanation is often a sign the code failed to express itself; mechanism/design explanations go to `docs/`; comments describe only current facts — corrected/discarded artifacts, sources, and evolution comparisons (e.g. "from research", "originally full-table fetch") are treated as never having existed
- **Tool-doc discipline**: tool docs (args/description) are the LLM interface (schemars-generated, injected into the agent) — write only what the schema cannot express (purpose/behavior/preconditions/format); no type-semantics ("(optional)" / "omitted = ..."), no field-name translation ("quantity limit" for `limit`), no defaults ("(default 10)"), no config paths (`[auxiliary.x]`); criterion: write only what the LLM would not know from the JSON schema (field names + types + required) alone
- **Git commit discipline**: atomic commits (one logical change per commit) + Conventional Commits prefix (`feat:`/`fix:`/`refactor:`/`docs:`/`chore:`/`perf:`); **commits are user-initiated — the agent only reminds** (when changes accumulate / across logical batches), never executes them; message language follows project style
- **External-action discipline**: creating/updating tracking entries and external content (issues/PRs/comments/backlog/ADR/decision records) is user-initiated or explicitly consented — never create/modify external-system (GitHub etc.) content on your own
- **Privacy & sensitive-info discipline**: credentials/keys/tokens, personal privacy, trade secrets, intranet/production sensitive info (production data cleanup / production config changes / credential handling) are **not written into the documentation system**; privacy info in existing records is removed; information sources are not recorded — comments/docs don't cite sources
</CRITICAL>

Full standards (cross-language engineering philosophy + Rust language craft): `h-agent-docs` skill `references/engineering-philosophy.md` + `references/rust-craft.md` — read for the rationale/details when the agent environment can.

## Testing

- Unit tests are few (`util/chunking.rs` 22 + `scheduler.rs` 3 regression tests); core behavior acceptance depends on manual runs (need real Telegram + Postgres; local environment usually lacks Telegram)
- `cargo test`; before delivery `cargo clippy --all-targets` must be warning-free

## Documentation navigation (trigger-read table)

<CRITICAL>
Before changing code, read the doc per the table — not a suggestion, mandatory.
</CRITICAL>

| Task type | Required doc |
|---|---|
| Session state machine / scheduling / chapter reopen / communication model | docs/topics/session.md |
| Prompt rendering / personality / skills | docs/topics/prompting.md |
| Tools / tool execution / multimodal / MCP | docs/topics/tools.md |
| Data model / service / vo / pgvector / migrations | docs/topics/domain.md |
| Config / error handling / Paths / provider | docs/topics/config.md |
| Telegram platform integration | docs/topics/platform.md |
| CLI / event log TUI / rebuild | docs/topics/cli.md |
| Cross-module wiring / event system / assembly root | docs/architecture.md |
| Term meanings (check before any domain code) | docs/CONTEXT.md |
| Changing a mechanism / refactor / new mechanism → also update the topic doc | docs/topics/<topic>.md (continuously maintained, same-turn update) |

## Documentation maintenance

- **Same-turn update**: any code change updates the corresponding `docs/` doc in the same turn; failing to do so = a delivery defect
- **Anchors**: all `file:symbol`, never line numbers
- **Change log**: none (Q1 decision) — changes that form a behavioral contract (observable behavior / interface / persisted format / domain semantics) go into the relevant `docs/topics/` topic doc; purely internal implementation (local refactor / typo / rename of a local variable) is not recorded; this file carries no change log
- **Terms**: resolved terms are written into `docs/CONTEXT.md` immediately, no batching
- **Mechanism/intent changes**: mechanism description & design intent → update the relevant `docs/topics/` topic doc (continuously maintained); not mixed into the code-fact docs
- **User project-level constraints**: user bans/norms/requirements → hardened into the Code style (hard rules) + the corresponding docs pitfall section
