ALTER TABLE "topic" DROP COLUMN "token_count";
ALTER TABLE "message" DROP COLUMN "token_count";
CREATE TABLE "conversation" (
    "chat_id" BIGINT NOT NULL,
    "messages" TEXT NOT NULL,
    "since_id" BIGINT NOT NULL,
    "run_count" INTEGER NOT NULL,
    "shown_memory_ids" TEXT NOT NULL,
    "shown_topic_ids" TEXT NOT NULL,
    "prompt_tokens" INTEGER NOT NULL,
    "last_turns" TEXT NOT NULL,
    "updated_at" TIMESTAMPTZ(6) NOT NULL,
    PRIMARY KEY ("chat_id")
);
