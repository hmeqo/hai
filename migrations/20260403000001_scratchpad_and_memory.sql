-- 草稿板表（固定格子模型）
CREATE TABLE scratchpad (
    chat_id BIGINT NOT NULL REFERENCES chat(id) ON DELETE CASCADE,
    slot INT NOT NULL CHECK (slot BETWEEN 0 AND 9),
    tag TEXT,
    content TEXT NOT NULL,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (chat_id, slot)
);

CREATE INDEX idx_scratchpad_expires ON scratchpad(expires_at) WHERE expires_at IS NOT NULL;

-- 长期记忆表结构优化
-- 新增 subject 字段（记忆主体）和 references 字段（关联引用）
ALTER TABLE memory
  ADD COLUMN subject TEXT,
  ADD COLUMN "references" JSONB;

-- 删除不再需要的独立字段，整合到 references 中
ALTER TABLE memory DROP COLUMN topic_id;
ALTER TABLE memory DROP COLUMN source_message_id;
ALTER TABLE memory DROP CONSTRAINT IF EXISTS memory_topic_id_fkey;
ALTER TABLE memory DROP CONSTRAINT IF EXISTS memory_source_message_id_fkey;
