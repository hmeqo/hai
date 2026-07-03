ALTER TABLE "memory" RENAME COLUMN "type" TO "kind";

UPDATE "memory" SET "meta" = "references" WHERE "references" IS NOT NULL;
DELETE FROM "memory" WHERE "kind" = 'rule';
UPDATE "memory" SET "kind" = 'note' WHERE "kind" = 'agent_note';

ALTER TABLE "memory" DROP COLUMN "subject";
ALTER TABLE "memory" DROP COLUMN "references";
ALTER TABLE "memory" DROP COLUMN "last_accessed_at";
