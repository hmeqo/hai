-- perception 改用 source JSONB（不再 FK resource），resource 表移除
-- Source::Resource → Source::Platform { platform, file_id }，格式不兼容，直接重建

DROP TABLE IF EXISTS perception;
DROP TABLE IF EXISTS resource;

CREATE TABLE perception (
    id              UUID          NOT NULL DEFAULT gen_random_uuid() PRIMARY KEY,
    source          JSONB         NOT NULL,
    parser          VARCHAR(32)   NOT NULL,
    prompt          TEXT,
    content         TEXT          NOT NULL DEFAULT '',
    embedding       VECTOR(1536),
    created_at      TIMESTAMPTZ   NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX idx_perception_unique_with_prompt
    ON perception(source, parser, prompt)
    WHERE prompt IS NOT NULL;
CREATE UNIQUE INDEX idx_perception_unique_no_prompt
    ON perception(source, parser)
    WHERE prompt IS NULL;
CREATE INDEX idx_perception_source ON perception(source);
CREATE INDEX idx_perception_embedding ON perception USING hnsw (embedding vector_cosine_ops);
