-- embedding 向量列不在此——由 util/pgvector.rs 的 ensure_embedding_schema 运行时补（游离于 schema）。

CREATE TABLE IF NOT EXISTS account (
    id              BIGSERIAL PRIMARY KEY,
    identity_id     UUID,
    platform        TEXT NOT NULL,
    external_id     TEXT NOT NULL,
    meta            JSONB,
    last_active_at  TIMESTAMPTZ NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_account_identity_id ON account (identity_id);
CREATE UNIQUE INDEX IF NOT EXISTS uq_account_platform_external ON account (platform, external_id);

CREATE TABLE IF NOT EXISTS chat (
    id          BIGSERIAL PRIMARY KEY,
    platform    TEXT NOT NULL,
    external_id TEXT NOT NULL,
    chat_type   TEXT NOT NULL,
    name        TEXT,
    meta        JSONB,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX IF NOT EXISTS uq_chat_platform_external ON chat (platform, external_id);

CREATE TABLE IF NOT EXISTS conversation (
    chat_id      BIGINT PRIMARY KEY,
    messages     JSONB NOT NULL,
    state        JSONB NOT NULL,
    context_meta JSONB NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS event (
    seq        BIGSERIAL PRIMARY KEY,
    domain     TEXT NOT NULL,
    payload    JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- payload 过滤（表达式索引须与查询字面一致：`(payload->>'chat_id')::bigint` / `payload->'payload'->>'event'`）
CREATE INDEX IF NOT EXISTS idx_event_payload_chat ON event (((payload->>'chat_id')::bigint));
CREATE INDEX IF NOT EXISTS idx_event_payload_event ON event (((payload->'payload'->>'event')));

CREATE TABLE IF NOT EXISTS identity (
    id         UUID PRIMARY KEY,
    name       TEXT,
    meta       JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS knowledge_chunk (
    id          UUID PRIMARY KEY,
    document_id UUID NOT NULL,
    seq         INT NOT NULL,
    content     TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (document_id, seq)
);

CREATE TABLE IF NOT EXISTS knowledge_document (
    id         UUID PRIMARY KEY,
    title      TEXT NOT NULL,
    collection TEXT NOT NULL,
    source     TEXT NOT NULL,
    content    TEXT NOT NULL,
    meta       JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- source 唯一（幂等导入比对依据）
CREATE UNIQUE INDEX IF NOT EXISTS uq_knowledge_document_source ON knowledge_document (source);

CREATE TABLE IF NOT EXISTS memory (
    id          UUID PRIMARY KEY,
    account_id  BIGINT,
    chat_id     BIGINT,
    kind        TEXT NOT NULL,
    content     TEXT NOT NULL,
    importance  INT NOT NULL,
    meta        JSONB,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- 向量检索 chat 过滤 + record_memory 查重
CREATE INDEX IF NOT EXISTS idx_memory_chat_id ON memory (chat_id);
CREATE INDEX IF NOT EXISTS idx_memory_dedup ON memory (kind, chat_id, content);

CREATE TABLE IF NOT EXISTS message (
    id                 BIGSERIAL PRIMARY KEY,
    chat_id            BIGINT NOT NULL,
    account_id         BIGINT,
    role               TEXT NOT NULL,
    content            JSONB NOT NULL,
    topic_id           UUID,
    interaction_status TEXT NOT NULL,
    reply_to_id        BIGINT,
    external_id        TEXT,
    meta               JSONB,
    sent_at            TIMESTAMPTZ,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_message_chat_id ON message (chat_id);
-- 幂等键查重（external_id 可空——部分唯一，NULL 不参与约束）
CREATE UNIQUE INDEX IF NOT EXISTS uq_message_chat_external ON message (chat_id, external_id) WHERE external_id IS NOT NULL;
-- 附件查找（find_attachment 的 JSONB @> 过滤）
CREATE INDEX IF NOT EXISTS idx_message_content_gin ON message USING GIN (content jsonb_path_ops);
-- topic 时间同步（sync_topic_times 按 topic_id 取最早/最晚）
CREATE INDEX IF NOT EXISTS idx_message_topic_id ON message (topic_id);

CREATE TABLE IF NOT EXISTS perception (
    id         UUID PRIMARY KEY,
    source     JSONB NOT NULL,
    parser     TEXT NOT NULL,
    prompt     TEXT,
    content    TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
-- 幂等键（source, parser, prompt）部分唯一：prompt 可空，拆两个部分索引
CREATE UNIQUE INDEX IF NOT EXISTS uq_perception_src_parser_prompt
    ON perception (source, parser, prompt) WHERE prompt IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS uq_perception_src_parser
    ON perception (source, parser) WHERE prompt IS NULL;

CREATE TABLE IF NOT EXISTS scratchpad (
    chat_id     BIGINT PRIMARY KEY,
    content     TEXT NOT NULL,
    token_count INT NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS topic (
    id               UUID PRIMARY KEY,
    chat_id          BIGINT NOT NULL,
    title            TEXT,
    summary          TEXT,
    status           TEXT NOT NULL,
    parent_topic_id  UUID,
    meta             JSONB,
    started_at       TIMESTAMPTZ NOT NULL,
    last_active_at   TIMESTAMPTZ NOT NULL,
    closed_at        TIMESTAMPTZ,
    created_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_topic_chat_id ON topic (chat_id);
-- 时间检索（by_chat_time：chat_id + status 过滤 + last_active_at 排序 + limit/offset 分页）
CREATE INDEX IF NOT EXISTS idx_topic_chat_status_active
    ON topic (chat_id, status, last_active_at DESC);

CREATE TABLE IF NOT EXISTS scheduled_task (
    id          UUID PRIMARY KEY,
    bot_id      TEXT NOT NULL,
    chat_id     BIGINT NOT NULL,
    description TEXT NOT NULL,
    fire_at     TIMESTAMPTZ NOT NULL,
    every_secs  BIGINT,
    is_active   BOOLEAN NOT NULL DEFAULT TRUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_scheduled_task_due ON scheduled_task (is_active, fire_at);
