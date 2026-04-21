-- 将 scratchpad 从 slot 模型改为 chat 一对一结构
-- 每个 chat 只有一条 scratchpad 记录，内容为 agent 的短期工作记忆

DROP TABLE IF EXISTS scratchpad;

CREATE TABLE scratchpad (
    chat_id     BIGINT      NOT NULL PRIMARY KEY REFERENCES chat(id) ON DELETE CASCADE,
    content     TEXT        NOT NULL DEFAULT '',
    token_count INT         NOT NULL DEFAULT 0,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
