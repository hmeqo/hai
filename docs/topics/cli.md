# CLI / Event log TUI / rebuild

> Command-line entry point, db management commands, event log TUI, and embedding rebuild. Required reading when changing `cli.rs`, `cli/`, `rebuild.rs`, or `domain/db.rs`.

## Overview

`hai` is the single binary. The CLI provides: starting the service, viewing configuration, db management, event log viewing/browsing (`log` TUI), and KnowledgeBase background management. For an overview of commands see the "Project custom commands" section in AGENTS.md (here we only record implementation semantics and parameter details).

## Design

### Command structure

Entry: `cli.rs:Cli` (clap Parser) + `cli.rs:Commands{Config, Db, Log, Kb}` + `DbAction{Create, Migrate, Rebuild}` + `RebuildTarget{Embeddings}` + `KbAction{Import, List, Search, Delete, Reindex}`; command implementation entry points: `cli/kb.rs:execute`, `cli/log.rs:execute`, `cli/tui/app.rs:run_tui`, `rebuild.rs:rebuild_embeddings`; `main.rs:main` = `Cli::parse().execute().await`.

All subcommands first load config: `AppConfigManager::from_file(Paths::inferred().config_file_str()).with_env(ENV_PREFIX)`.

### db commands (cli.rs + domain/db.rs)

| Command | Behavior |
|---|---|
| `db create` | `domain/db.rs:create_database(url)`: splits the admin URL to connect to the `postgres` database and create the database (`split_admin_url`) |
| `db migrate` | `init_db` + `run_migrations` (schema.sql idempotent table creation) + `ensure_embedding_schema(dimension defaults to 1024)` — **migrations are not auto-applied** (must be run manually after deployment) |
| `db rebuild embeddings` | See below |

### rebuild embeddings (rebuild.rs:rebuild_embeddings)

```txt
init_db → ProviderRegistry::new(config)
provider = embedding.provider_or(&agent.provider); model = embedding.model(); dimension = embedding.dimension() (defaults to 1024)
ApiClient::new + registry.get_endpoint(provider, model)   # connect directly to provider, not through the EmbeddingService trait
ensure_embedding_schema(dim)
reset_embeddings: DROP INDEX idx_{t}_embedding + UPDATE ... SET embedding=NULL (4 tables)
collect_jobs: Memory all; Topic only summary non-empty; Perception all; KnowledgeChunk all
run_batch: Semaphore(MAX_CONCURRENT=10) + FuturesUnordered + indicatif progress bar
  client.embed → UPDATE {table} SET embedding='{vec}'::vector WHERE id='{id}' (sqlx bind parameters)
rebuild_indexes: CREATE INDEX ... USING ivfflat (embedding vector_cosine_ops) WITH (lists = max(10, sqrt(total)))
```

Failed count > 0 → `Err(Internal)` (with `{failed}/{total}` count). Index lifecycle see docs/topics/domain.md.

### Event log (log command + TUI)

- `cli/log.rs:LogArgs{id?, chat?, event?}`; `--id N` → `cli/log.rs:show_detail` (`repos.event.by_seq(seq)` + `EventDisplay`, prints `time chat tag` header + separator line + detail_text); otherwise `run_tui(db, chat, event)`
- `--event` help text uses kebab-case examples (`turn-started, tool-call, turn-ended` etc.), consistent with the stored tag

**Rendering (cli/display.rs)**:

- `EventDisplay{tag, one_liner, detail_text, chat_id, color}`: generates a one-line summary + detail per the 9 AgentEventPayload variants; `tag_for_kind`: TURN/TOOL/STEP/DONE/RETRY/FAIL/STEER/WRAP (TOOL covers tool-call and tool-call-result, WRAP covers wrap-up-started and wrap-up-completed — the storage layer only has a single turn-ended tag; FAIL/STEER filtering is currently equivalent to DONE, see Pitfalls)
- `color_rgb(payload)` colors by event type; `fmt_time` (same-day `%H:%M:%S`, cross-day `%m-%d %H:%M`, local timezone); `chat_display` (`{:+}` with sign)
- wrap-up event display: `WrapUpStarted` → `WRAP start`; `WrapUpCompleted{step_count}` → `WRAP N steps`; `WrapUpFailed{error}` → `FAIL wrap-up {error}`
- event queries go through `domain/repo/event.rs:EventRepo::query`: dynamic SQL (conditions concatenated with `$N` placeholders) + JSONB filter `(payload::jsonb->>'chat_id')::bigint` / `payload::jsonb->>'event'`, parameterized bind

### TUI (cli/tui/)

`cli/tui/app.rs:run_tui(db, chat_filter, kind_filter) -> crate::error::Result<()>` (ratatui + crossterm, no anyhow). Three-panel componentization: `App` (loop + dispatch, holds store/focus/three components/pending queue) + `Filter` + `Events` + `Detail`; components do not hold data, they communicate via the `app.rs:Cmd` enum — synchronous commands execute immediately, async (reload-like) ones go into the pending queue and are consumed on tick.

**Layout**: left column 60% vertically split into **filter** (top, 5 rows including border) + **events** (bottom); right column 40% **detail**; Enter in the detail area toggles fullscreen (`DetailLayout`). On startup it loads the latest events and opens detail.

**filter panel (Filter, three `FilterRow` rows)**: text row input filters immediately (local window), Backspace triggers rebuild; chat row numeric input submitted with Enter (DB reload); type row `(all)` + 8 type horizontal shortcuts (TURN/TOOL/STEP/DONE/RETRY/FAIL/STEER/WRAP, `display.rs:KIND_TAGS`), ←/→ cycles and applies immediately; row focus j/k (when the text row is focused j/k are character input, only ↓ switches row).

**Event list and stepless scrolling (Events)**: anchored by `visible_seq` + `selected_seq` (window changes restore by seq without drift); the selected row stays in the upper 1/3 of the viewport (`clamp_offset`, scrolloff = vp_h/3); PgUp/Dn = half page, Ctrl+U/D = 1/4 page; scrolling to the top auto-loads older events (`visual_pos` + `anchor_after_prepend` visual anchoring, `back_loading` prevents duplicates).

**Scroll-driven loading**: `app.rs:POLL_MS`=200 tick polls `event_store.rs:count_new` (only counts matching events with seq > max) → header `+N new (G to load)`; at the bottom with new events → append load (`Cmd::AppendNew`); `events.rs:auto_loaded` prevents automatically catching up continuously when stopped at the bottom; when not in follow mode new events only show +N new, press G to load the latest.

**Keys (vim style, reused by focus context)**:

| Key | filter | events | detail |
|---|---|---|---|
| j/k | row switch (except text row) | navigation | scroll |
| ←/→ | text/chat cursor / type switch | — | — |
| Tab / l | → Events | → Detail | → Filter |
| h | — | → Filter | → Events |
| Enter | chat submit | open/close detail | fullscreen toggle |
| Esc | cancel chat edit | close detail / clear error | close detail |
| g / G | — | earliest / latest | — |
| PgUp/PgDn | — | half page | 10 rows |
| Ctrl+U / Ctrl+D | — | 1/4 page | 15 rows |
| `/` / `c` | — | jump to text row / jump to chat row | — |
| `?` | help popup (global) | | |
| q / Ctrl+C | exit (q is a character while typing text; Ctrl+C always exits) | | |

Esc priority: clear error → cancel chat row edit → close detail (`app.rs:handle_key` tries them in order).

**Data layer (event_store.rs:EventStore)**: seq-ascending growing window — neither `extend_back` (prepend) nor `append_new` (append) trims (keeps selection and continued loading while browsing history; session-level memory); `set_viewport` makes capacity = max(lines*3, 400), only constraining a single query's limit; boundaries are determined by "query returns 0 rows" (`at_start`/`at_end`), transient errors do not pollute boundaries; `display_by_seq` parse cache (each event deserialized only once, cache cleaned when exceeding capacity×4).

**Terminal safety and rendering**: `TerminalGuard` RAII (Drop restores raw mode + alternate screen); crossterm `EventStream` + dirty-flag redraw (draws only on change, no 100Hz idle spin); header `hai Chat +N X/Y events`; bottom error bar in red (cleared with Esc).

**Filtering & DB**: chat/type filtering is AND-combined at the DB layer (`load_end` re-queries); text filtering is in the local window (one_liner + seq number + chat_id numeric string, case-insensitive); **the SQL for chat/type filtering and count_new all use the `payload::jsonb` cast** — the payload column is TEXT/JSONB dual-form compatible, `->>` only applies to jsonb (see Pitfalls).

## Boundaries

- CLI and library are in the same crate (db management logic can be shared between app and CLI)
- No event editing/deletion/export; no multi-session (multiple chats at once) view; no migration of historical log data

## Pitfalls / common mistakes

- rebuild inserts vectors with sqlx bind (`$1::vector` — value formatted from Vec<f32>, parameterized binding)
- **the payload column is JSONB** (phase 2 refactor: TEXT→JSONB, `payload->>` works directly; `raw_query` and `count_new` have removed the cast — if reverting to TEXT, the explicit cast must be restored)
- **sqlx placeholders `$N`** (Postgres dialect) — hand-written SQL numbers binds in condition order (dynamic SQL is wrapped with `sqlx::AssertSqlSafe`)
- **raw_query condition fragments start with "AND " but the join adds no space**: a space must be added before the first condition, otherwise it concatenates to `WHERE seq > 0AND…`
- Legacy: the event log window grows with the session and is not trimmed (session-level memory; watch out for very long runs + high-frequency events); `db migrate` currently adds query indexes / idempotent key constraints (in schema.sql), while the embedding IVFFlat index is still built by `db rebuild embeddings` (l2_ops)
