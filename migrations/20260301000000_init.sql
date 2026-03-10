-- 启用扩展
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- 自动更新 updated_at 的触发器函数
CREATE OR REPLACE FUNCTION update_updated_at_column()
RETURNS TRIGGER AS $$
BEGIN
    NEW.updated_at = CURRENT_TIMESTAMP;
    RETURN NEW;
END;
$$ language 'plpgsql';

-- ==========================================
-- 1. 身份与账号 (Identity & Platform Account)
-- ==========================================

CREATE TABLE IF NOT EXISTS identity (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name VARCHAR(100),              -- 统一称呼
    meta JSONB,                     -- 全局配置/偏好/画像
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER update_identity_updated_at BEFORE UPDATE ON identity FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

CREATE TABLE IF NOT EXISTS account (
    id BIGSERIAL PRIMARY KEY,
    identity_id UUID REFERENCES identity(id) ON DELETE SET NULL, -- 关联真实身份 (允许为空，支持渐进式绑定)
    platform VARCHAR(20) NOT NULL,            -- 'telegram', 'qq', 'system'
    external_id TEXT NOT NULL,                -- 平台原始 ID
    meta JSONB,                               -- 头像、昵称等平台特定信息
    last_active_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(platform, external_id)
);

CREATE TRIGGER update_account_updated_at BEFORE UPDATE ON account FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE INDEX IF NOT EXISTS idx_account_identity ON account(identity_id);

-- ==========================================
-- 2. 场所 (Chat)
-- ==========================================

CREATE TABLE IF NOT EXISTS chat (
    id BIGSERIAL PRIMARY KEY,
    platform VARCHAR(20) NOT NULL,
    external_id TEXT NOT NULL,      -- 平台原始 Chat/Group ID
    chat_type VARCHAR(20) NOT NULL DEFAULT 'group', -- 'private', 'group', 'channel'
    name TEXT,
    config JSONB,                   -- Agent在该群的配置/人设/规则
    meta JSONB,                     -- 群头像、人数等
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(platform, external_id)
);

CREATE TRIGGER update_chat_updated_at BEFORE UPDATE ON chat FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();

-- ==========================================
-- 3. 话题流 (Topic)
-- ==========================================

CREATE TABLE IF NOT EXISTS topic (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    chat_id BIGINT NOT NULL REFERENCES chat(id) ON DELETE CASCADE,
    title TEXT,
    summary TEXT,
    embedding VECTOR(1536),
    
    status VARCHAR(20) DEFAULT 'active', -- 'active', 'closed', 'paused'
    parent_topic_id UUID REFERENCES topic(id) ON DELETE SET NULL,
    
    token_count INT DEFAULT 0,
    message_count INT DEFAULT 0,
    meta JSONB,                     -- 额外信息
    
    started_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    last_active_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    closed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER update_topic_updated_at BEFORE UPDATE ON topic FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE INDEX IF NOT EXISTS idx_topic_chat_active ON topic(chat_id) WHERE status = 'active';
CREATE INDEX IF NOT EXISTS idx_topic_embedding ON topic USING hnsw (embedding vector_cosine_ops);

-- ==========================================
-- 4. 消息流 (Message)
-- ==========================================

CREATE TABLE IF NOT EXISTS message (
    id BIGSERIAL PRIMARY KEY,
    chat_id BIGINT NOT NULL REFERENCES chat(id) ON DELETE CASCADE,
    
    -- 关联到 account_id (用户或 Agent)
    account_id BIGINT REFERENCES account(id) ON DELETE SET NULL, 
    
    role VARCHAR(20) NOT NULL,      -- 'user', 'assistant', 'system', 'tool'
    content JSONB NOT NULL,         -- 结构化内容 (Array of ContentPart)
    
    topic_id UUID REFERENCES topic(id) ON DELETE SET NULL,
    
    -- 互动状态 (Interaction Status): 'pending', 'seen', 'replied', 'ignored'
    interaction_status VARCHAR(20) NOT NULL DEFAULT 'pending',
    
    reply_to_id BIGINT REFERENCES message(id) ON DELETE SET NULL, -- 引用消息 ID (内部)
    external_id TEXT,               -- 平台原始 Message ID
    meta JSONB,                     -- 存储模型名称、消耗的 token 等
    token_count INT,                -- 消息本身的 token 数量
    
    sent_at TIMESTAMPTZ,            -- 平台原始发送时间
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER update_message_updated_at BEFORE UPDATE ON message FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE INDEX IF NOT EXISTS idx_message_interaction ON message(chat_id, interaction_status);
CREATE INDEX IF NOT EXISTS idx_message_topic ON message(topic_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_message_external ON message(chat_id, external_id) WHERE external_id IS NOT NULL;

-- ==========================================
-- 5. 统一记忆库 (Memory)
-- ==========================================

CREATE TABLE IF NOT EXISTS memory (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    
    -- 作用域 (至少有一个不为空)
    account_id BIGINT REFERENCES account(id) ON DELETE CASCADE, -- 关于某人的记忆
    chat_id BIGINT REFERENCES chat(id) ON DELETE CASCADE,       -- 关于某群的记忆
    topic_id UUID REFERENCES topic(id) ON DELETE CASCADE,       -- 关于某话题的笔记
    
    -- 类型: 'user_fact', 'agent_note', 'knowledge', 'summary', 'rule'
    type VARCHAR(32) NOT NULL,
    
    content TEXT NOT NULL,
    embedding VECTOR(1536),
    
    source_message_id BIGINT REFERENCES message(id) ON DELETE SET NULL,
    
    importance INT DEFAULT 1,       -- 记忆重要度 (1-10)
    meta JSONB,                     -- 记忆的额外属性
    
    last_accessed_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    created_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
);

CREATE TRIGGER update_memory_updated_at BEFORE UPDATE ON memory FOR EACH ROW EXECUTE FUNCTION update_updated_at_column();
CREATE INDEX IF NOT EXISTS idx_memory_vec ON memory USING hnsw (embedding vector_cosine_ops);
CREATE INDEX IF NOT EXISTS idx_memory_account ON memory(account_id);
CREATE INDEX IF NOT EXISTS idx_memory_chat ON memory(chat_id);
CREATE INDEX IF NOT EXISTS idx_memory_type ON memory(type);
