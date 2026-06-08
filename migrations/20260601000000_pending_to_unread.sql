-- 将 interaction_status 从 pending 迁移到 unread
UPDATE message SET interaction_status = 'unread' WHERE interaction_status = 'pending';
