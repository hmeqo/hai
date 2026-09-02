use serde_json::Value;
use sqlx::PgPool;

use crate::{domain::model::ConversationRecord, error::Result};

#[derive(Debug, Clone)]
pub struct ConversationRecordRepo {
    pool: PgPool,
}

impl ConversationRecordRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// filter 查询：无行返回 `Ok(None)`（而非 not-found 错误），service 借此区分
    /// "无记录"与"DB 违约"。
    pub async fn find_by_chat_id(&self, chat_id: i64) -> Result<Option<ConversationRecord>> {
        sqlx::query_as::<_, ConversationRecord>(
            "SELECT * FROM conversation WHERE chat_id = $1 LIMIT 1",
        )
        .bind(chat_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn create(
        &self,
        chat_id: i64,
        messages: Value,
        state: Value,
        context_meta: Value,
        updated_at: jiff::Timestamp,
    ) -> Result<ConversationRecord> {
        sqlx::query_as::<_, ConversationRecord>(
            "INSERT INTO conversation (chat_id, messages, state, context_meta, updated_at) \
             VALUES ($1, $2, $3, $4, $5) RETURNING *",
        )
        .bind(chat_id)
        .bind(messages)
        .bind(state)
        .bind(context_meta)
        .bind(jiff_sqlx::Timestamp::from(updated_at))
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn update(
        &self,
        chat_id: i64,
        messages: Value,
        state: Value,
        context_meta: Value,
        updated_at: jiff::Timestamp,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE conversation SET messages = $2, state = $3, context_meta = $4, updated_at = $5 \
             WHERE chat_id = $1",
        )
        .bind(chat_id)
        .bind(messages)
        .bind(state)
        .bind(context_meta)
        .bind(jiff_sqlx::Timestamp::from(updated_at))
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
