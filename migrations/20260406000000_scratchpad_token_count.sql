ALTER TABLE scratchpad ADD COLUMN token_count INT NOT NULL DEFAULT 0;

-- 回填现有数据的 token_count（简单估算：content 长度的 1/4）
UPDATE scratchpad SET token_count = LENGTH(content) / 4;
