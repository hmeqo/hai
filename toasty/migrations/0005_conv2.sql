ALTER TABLE "conversation" DROP COLUMN "run_count";
ALTER TABLE "conversation" DROP COLUMN "prompt_tokens";
ALTER TABLE "conversation" RENAME COLUMN "last_turns" TO "turns";
