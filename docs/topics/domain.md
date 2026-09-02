# Data layer (domain)

> 13 model tables, 13 repos (sqlx), 10 services, value objects, pgvector, migrations. Required reading when changing `domain/`, `util/pgvector.rs`, `util/chunking.rs`, `migrations/`.

## Overview

domain is the data layer: model (data carriers) + repo (sqlx query wrappers) + service (business logic). **service exposes zero sqlx types** — all queries go through repo. The embedding vector columns live outside schema.sql and are maintained at runtime by `util/pgvector.rs`.

## Design

### Model overview (13 tables, `domain/model/`)

> Field definitions follow `domain/model/*.rs` (grep gives them); this section records only semantics that code cannot reveal.

| Model (definition file) | Table | semantics |
|---|---|---|
| `domain/model/memory.rs:Memory` | memory | kind three-way classification user_fact/note/knowledge; importance fixed to 1 on create |
| `domain/model/topic.rs:Topic` | topic | parent/child hierarchy has no code write path; message_count always 0 (no update path); no reopen capability |
| `domain/model/message.rs:Message` | message | external_id is the idempotency key; interaction_status has two states (unread/seen) |
| `domain/model/perception.rs:Perception` | perception | upsert key = source+parser+focus; focus's actual column name is prompt (None = base transcription row / Some = targeted judgment row) |
| `domain/model/conversation_record.rs:ConversationRecord` | conversation | context_messages + state{since_id} + context_meta{shown ids, tokens, turn_count, step_count} persisted (state metadata grouped into two columns, see docs/topics/session.md) |
| `domain/model/account.rs:Account` | account | last_active_at refreshed on the message path; Platform includes the unused Qq variant |
| `domain/model/chat.rs:Chat` | chat | config always None (reserved, unused) |
| `domain/model/event.rs:Event` | event | no chat_id/kind columns; payload is JSONB; domain always "agent" |
| `domain/model/identity.rs:Identity` | identity | — |
| `domain/model/scratchpad.rs:Scratchpad` | scratchpad | reserved feature (skeleton kept for later implementation, don't delete); token_count always 0 |
| `domain/model/scheduled_task.rs:ScheduledTask` | scheduled_task | bot_id+chat_id scope; every_secs None = one-shot / Some = recurring; next_fire_after skips past moments and takes the first future trigger point |
| `domain/model/knowledge_document.rs:KnowledgeDocument` | knowledge_document | knowledge base document (lifecycle unit); empty collection = uncategorized; meta stores file_hash + chunker_version |
| `domain/model/knowledge_chunk.rs:KnowledgeChunk` | knowledge_chunk | retrieval unit; UNIQUE(document_id, seq) keeps chunk ordering stable |

Relationships (belongs_to): Account→Identity; Memory→Account/Chat; Message→Account/Chat/Topic (reply_to self-reference); Topic→Chat (parent self-reference); ConversationRecord→Chat; Scratchpad→Chat.

### Service overview (10 services, `domain/service/`; the `DbServices` assembler is in mod.rs)

> Method signatures follow the code; this section records only responsibilities and behavior the code cannot reveal.

| Service (definition file) | responsibility |
|---|---|
| `domain/service/memory.rs:MemoryService` | memory CRUD + embedding (create dedupe → embed → insert row; update re-embeds on content change; search returns all three kinds together via semantic vector retrieval; delete removes row + clears vector) |
| `domain/service/topic.rs:TopicService` | topic management (create is a transaction: create topic + attach message + mark Seen; append_summary has an ensure_not_closed guard; update_topic does not check closed; close_topic writes the vector; semantic search only recalls closed) |
| `domain/service/message.rs:MessageService` | message persistence + cursor reading (unread first + backfill; mark_unread_seen only on unread rows; find_attachment full scan) |
| `domain/service/perception.rs:PerceptionService` | perception cache (upsert key source+parser+focus; async embedding best-effort) |
| `domain/service/conversation_record.rs:ConversationRecordService` | conversation snapshot persistence (get is contractual: Ok(None) = no record / Err propagates without creating a session; save/restore pub(crate)) |
| `domain/service/platform.rs:PlatformService` | account/chat registration (hit does not refresh last_active_at; get_* swallows errors) |
| `domain/service/identity.rs:IdentityService` | identity binding (reserved) |
| `domain/service/scratchpad.rs:ScratchpadService` | reserved feature (skeleton kept for later implementation; zero callers, get swallow-error is legacy) |
| `domain/service/scheduled_task.rs:ScheduledTaskService` | scheduled tasks (create/list_active/list_all/cancel sets inactive/due queries due items/advance moves fire_at forward — recurring skips past moments, one-shot becomes inactive) |
| `domain/service/knowledge.rs:KnowledgeService` | knowledge base (sha256 idempotent: same source + same hash skipped / different hash replaces the whole document and rebuilds; force=true skips the idempotency check (for reindex); embeddings generated concurrently outside the transaction (buffered(10) preserves order), pure DB writes inside the transaction, failure propagates and rolls back leaving no bad data; delete cascades chunk removal; search does global retrieval + collections whitelist + limit clamp 1..=1000; reindex compares by chunker version, warns and triggers a rebuild when meta parsing is corrupted) |
| `domain/service/mod.rs:DbServices` | assembly: injects MultimodalService as `Arc<dyn EmbeddingService>` into memory/topic/perception/knowledge |

### Value objects (`domain/vo/`)

- `id.rs` — `id_type!` yields 11 newtypes (9 entity IDs + TurnNumber/StepNumber; transparent wrapper + bidirectional From + serde transparent)
- `content.rs` — `TelegramContentPart` (serde tag="type", 9 kinds of parts) + AttachmentParser{Image,Audio,Video,Ocr} + MediaCodec + FileId
- `resource.rs` — `Source{Platform | Url}` + `resource_id_from_file_id` (UUIDv5, perception dedupes the same resource)
- `turn.rs` / `tool_call_result.rs` — Turn / ToolCallResult (ok()/err())
- `event.rs` — `AgentEventPayload` (9 variants, tag="event", kebab-case, includes WrapUpStarted/WrapUpCompleted{step_count}/WrapUpFailed{error}) + ModelRetryReason + TurnOutput — see docs/architecture.md "Event definition and persistence"
- `conversation_snapshot.rs` — snapshot VO (the dependency on genai::chat::ChatMessage is an exception)
- `topic.rs` / `meta.rs` — TopicSearchResult / MessageMeta (serde flatten)

### Chunking (`util/chunking.rs`, pure functions, zero dependencies)

Knowledge base document chunking: deterministic (same input, same output, the foundation of idempotency). `ChunkCfg{size=512, overlap=51, max=1536}` (from the `[knowledge]` config; the default values come from chunking-strategy research: bge-m3 + Chinese RAG practice).

- **Structure first**: Markdown heading tree → section (each chunk carries a heading-path prefix as a semantic anchor); within a section, aggregate by structural units, targeting `size`, with a hard cap `max` (overlong units are split internally)
- **Protection rules**: code fences are never cut (closed fences tolerate leading whitespace); table rows are never cut (overlong sub-blocks repeat the header); list items are never cut; sentence boundaries are preferred, with character fallback (char safe)
- **overlap**: adjacent chunks carry over 10% (falling back to sentence boundaries); code/table/list boundaries are skipped; across a section (heading boundary) it is skipped
- Overlong documents are rejected for import (`MAX_CHUNKS_PER_DOC` = 10_000, no silent truncation); covered by 22 unit tests

### pgvector (`util/pgvector.rs`, raw SQL, no pgvector crate)

- Formatting: `vec_to_pgstring` produces `[0.1,0.2,...]` literals
- Schema maintenance: `ensure_embedding_schema` (CREATE EXTENSION vector + adds/changes columns on memory/topic/perception/knowledge_chunk, idempotent; shared by db migrate and rebuild)
- Retrieval: `search_embedding_vec` — `<->` L2 distance + `embedding IS NOT NULL` (under unit-normalized embeddings L2 and cosine give the same ranking; knowledge_chunk retrieval also uses `<->`); **chat_id: Some = filter by chat (memory/topic), None = global retrieval (knowledge_chunk)**; extra_filter is a constant SQL fragment, interpolation of user input is forbidden
- Writes: `upsert_embedding_vec` / `clear_embedding_vec` (single-row update/clear); `store_embedding` = generate + upsert in one place (callers: topic.close_topic, perception upsert async path)

Embedding write timing is differentiated: memory writes synchronously (failure propagates); topic.close_topic only warns on embedding failure (close already committed; missing vectors cannot be recalled by semantic search — a known trade-off); perception spawns asynchronously (best-effort); knowledge writes inside the transaction (via `pool.begin()` within the repo method — a pool connection cannot see uncommitted rows, so it must go through tx).

### Migrations (sqlx)

- `migrations/schema.sql`: 12 tables with CREATE TABLE IF NOT EXISTS + indexes (idempotent — already-deployed databases skip, new databases are fully built); `domain/db.rs:run_migrations` is inlined in `db migrate`
- **embedding columns are not in schema.sql**: `db migrate` adds the columns + creates query indexes / idempotent-key unique constraints (within schema.sql); **the IVFFlat vector index is only built by `db rebuild embeddings`** (`vector_l2_ops` — must match the query operator `<->` L2, do not change to cosine_ops)

## Boundaries

- Field lists / interface signatures are not repeated here (grep gives them) — only semantics and behavior the code cannot reveal
- The repo layer (`domain/repo/`) encapsulates all SQL — service exposes zero sqlx types (the connection pool is passed to the pgvector util via `Repos::pool()`); transaction boundaries live inside repo methods
- Conversation state lives forever; no expiry-based deletion is performed

## Pitfalls / common mistakes

- **Layering violation**: `domain/service/mod.rs` (`DbServicesInner`) depends on `agent::multimodal::MultimodalService` (EmbeddingService injection) — see docs/architecture.md; when changing the assembly, keep domain from depending back on agent
- `create` always writes `chat_id: Some(...)` for all three Memory kinds (Knowledge actually has a chat_id); vector search always filters by chat_id — a future Knowledge with chat_id=NULL would not be recalled ([INFERENCE])
- **No reopen capability**: after close, `update_topic` does not change status; no reopen tool exists
- **Swallow-error pattern**: `get_message_by_id` / `get_chat_by_id` / `get_account_by_id` / `ScratchpadService::get` swallow errors into `Ok(None)` — do not spread this pattern (`ConversationRecordService::get` has been fixed to be contractual)
- `find_attachment`: JSONB `@>` pushdown (`idx_message_content_gin`)
- Unique constraints already added: `uq_message_chat_external` / `uq_account_platform_external` / `uq_chat_platform_external` / `uq_knowledge_document_source` / perception partial uniques (two: `prompt IS NULL` / `IS NOT NULL`) — concurrent double-writes on idempotency keys are guarded by the DB layer
- Json<T> columns are stored as TEXT (not real JSONB); `ALTER TYPE` to change dimensions rewrites the whole table (slow on large tables, [INFERENCE])
- Legacy: `Platform::Qq` is a zero-reference variant (kept); the event table may retain an orphan created_at index built in 0002 ([INFERENCE]). Already cleaned up: `Topic::message_count`/`Chat::config` columns dropped, `Account::last_active_at` wired to refresh (message path), `TopicId` newtype actually used (topic service signatures), event.payload TEXT→JSONB

## Evolution direction

- Fix the layering violation: sink MultimodalService down or isolate the embedding trait (see docs/architecture.md "Evolution direction")
