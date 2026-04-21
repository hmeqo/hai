-- Fix topic started_at and last_active_at NOT NULL constraint

ALTER TABLE topic ALTER COLUMN started_at SET NOT NULL;
ALTER TABLE topic ALTER COLUMN last_active_at SET NOT NULL;

-- Fix existing NULL values
UPDATE topic 
SET started_at = COALESCE(started_at, CURRENT_TIMESTAMP),
    last_active_at = COALESCE(last_active_at, CURRENT_TIMESTAMP)
WHERE started_at IS NULL OR last_active_at IS NULL;
