use serde_json::Value;
use sqlx::PgPool;

use crate::{domain::model::Message, error::Result};

#[derive(Debug, Clone)]
pub struct MessageRepo {
    pool: PgPool,
}

impl MessageRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn find_by_chat_external_id(
        &self,
        chat_id: i64,
        external_id: &str,
    ) -> Result<Option<Message>> {
        sqlx::query_as::<_, Message>(
            "SELECT * FROM message WHERE chat_id = $1 AND external_id = $2 LIMIT 1",
        )
        .bind(chat_id)
        .bind(external_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        chat_id: i64,
        account_id: Option<i64>,
        role: &str,
        content: Value,
        topic_id: Option<uuid::Uuid>,
        interaction_status: &str,
        reply_to_id: Option<i64>,
        external_id: Option<&str>,
        meta: Value,
        sent_at: Option<jiff::Timestamp>,
    ) -> Result<Message> {
        sqlx::query_as::<_, Message>(
            "INSERT INTO message (chat_id, account_id, role, content, topic_id, \
             interaction_status, reply_to_id, external_id, meta, sent_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING *",
        )
        .bind(chat_id)
        .bind(account_id)
        .bind(role)
        .bind(content)
        .bind(topic_id)
        .bind(interaction_status)
        .bind(reply_to_id)
        .bind(external_id)
        .bind(meta)
        .bind(sent_at.map(jiff_sqlx::Timestamp::from))
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn update_content_meta_status(
        &self,
        id: i64,
        content: Value,
        meta: Value,
        interaction_status: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE message SET content = $2, meta = $3, interaction_status = $4 WHERE id = $1",
        )
        .bind(id)
        .bind(content)
        .bind(meta)
        .bind(interaction_status)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn by_chat_status_desc(
        &self,
        chat_id: i64,
        status: &str,
        limit: Option<i64>,
    ) -> Result<Vec<Message>> {
        let mut sql = String::from(
            "SELECT * FROM message WHERE chat_id = $1 AND interaction_status = $2 ORDER BY id DESC",
        );
        if let Some(l) = limit {
            sql.push_str(&format!(" LIMIT {l}"));
        }
        let q = sqlx::query_as::<_, Message>(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(chat_id)
            .bind(status);
        let rows = q.fetch_all(&self.pool).await?;
        Ok(rows)
    }

    pub async fn by_chat_status_ne_desc(
        &self,
        chat_id: i64,
        status: &str,
        limit: Option<i64>,
    ) -> Result<Vec<Message>> {
        let mut sql = String::from(
            "SELECT * FROM message WHERE chat_id = $1 AND interaction_status != $2 ORDER BY id DESC",
        );
        if let Some(l) = limit {
            sql.push_str(&format!(" LIMIT {l}"));
        }
        let rows = sqlx::query_as::<_, Message>(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(chat_id)
            .bind(status)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    pub async fn by_chat_after_desc(&self, chat_id: i64, since_id: i64) -> Result<Vec<Message>> {
        let rows = sqlx::query_as::<_, Message>(
            "SELECT * FROM message WHERE chat_id = $1 AND id > $2 ORDER BY id DESC",
        )
        .bind(chat_id)
        .bind(since_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    pub async fn by_id(&self, id: i64) -> Result<Option<Message>> {
        sqlx::query_as::<_, Message>("SELECT * FROM message WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn by_ids(&self, ids: &[i64]) -> Result<Vec<Message>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows =
            sqlx::query_as::<_, Message>("SELECT * FROM message WHERE id = ANY($1) ORDER BY id")
                .bind(ids)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows)
    }

    pub async fn id_by_chat_external_id(
        &self,
        chat_id: i64,
        external_id: &str,
    ) -> Result<Option<i64>> {
        let id: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM message WHERE chat_id = $1 AND external_id = $2 LIMIT 1",
        )
        .bind(chat_id)
        .bind(external_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn count_unread_by_chat(&self, chat_id: i64) -> Result<i64> {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM message WHERE chat_id = $1 AND interaction_status = 'unread'",
        )
        .bind(chat_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(n)
    }

    pub async fn mark_unread_seen(&self, ids: &[i64]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        sqlx::query(
            "UPDATE message SET interaction_status = 'seen' \
             WHERE id = ANY($1) AND interaction_status = 'unread'",
        )
        .bind(ids)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_meta(&self, id: i64, meta: Option<Value>) -> Result<()> {
        sqlx::query("UPDATE message SET meta = $2 WHERE id = $1")
            .bind(id)
            .bind(meta)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// 按附件 UUID 查找消息（JSONB `@>` 过滤，最近一条；GIN 索引加速）。
    pub async fn find_by_attachment(&self, needle: &serde_json::Value) -> Result<Option<Message>> {
        sqlx::query_as::<_, Message>(
            "SELECT * FROM message WHERE content @> $1 ORDER BY id DESC LIMIT 1",
        )
        .bind(needle)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }
}
