use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{domain::model::Topic, error::Result};

#[derive(Debug, Clone)]
pub struct TopicRepo {
    pool: PgPool,
}

impl TopicRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 创建 topic 并把消息挂载到该 topic（同一事务；message_ids 可空）。
    pub async fn create_with_messages(
        &self,
        chat_id: i64,
        title: &str,
        summary: &str,
        parent_topic_id: Option<Uuid>,
        message_ids: &[i64],
        meta: Option<Value>,
    ) -> Result<Topic> {
        let mut tx = self.pool.begin().await?;
        let now = jiff_sqlx::Timestamp::from(jiff::Timestamp::now());
        let id = Uuid::now_v7();

        let topic = sqlx::query_as::<_, Topic>(
            "INSERT INTO topic (id, chat_id, title, summary, status, parent_topic_id, meta, \
             started_at, last_active_at, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, 'active', $5, $6, $7, $7, $7, $7) RETURNING *",
        )
        .bind(id)
        .bind(chat_id)
        .bind(title)
        .bind(summary)
        .bind(parent_topic_id)
        .bind(meta)
        .bind(now)
        .fetch_one(&mut *tx)
        .await?;

        if !message_ids.is_empty() {
            sqlx::query(
                "UPDATE message SET topic_id = $2, interaction_status = 'seen' \
                 WHERE id = ANY($1)",
            )
            .bind(message_ids)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(topic)
    }

    /// 把消息挂到 topic，并按该 topic 最早/最晚消息刷新 started_at/last_active_at（同一事务）。
    pub async fn assign_messages(&self, message_ids: &[i64], topic_id: Uuid) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        if !message_ids.is_empty() {
            sqlx::query(
                "UPDATE message SET topic_id = $2, interaction_status = 'seen' \
                 WHERE id = ANY($1)",
            )
            .bind(message_ids)
            .bind(topic_id)
            .execute(&mut *tx)
            .await?;
        }

        sync_topic_times(&mut tx, topic_id).await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn by_id(&self, id: Uuid) -> Result<Option<Topic>> {
        sqlx::query_as::<_, Topic>("SELECT * FROM topic WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn by_ids(&self, ids: &[Uuid]) -> Result<Vec<Topic>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, Topic>("SELECT * FROM topic WHERE id = ANY($1)")
            .bind(ids)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows)
    }

    /// 按需更新 title / summary（传入 Some 才覆盖对应列）。
    pub async fn update_fields(
        &self,
        topic_id: Uuid,
        title: Option<&str>,
        summary: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE topic SET title = COALESCE($2, title), summary = COALESCE($3, summary) \
             WHERE id = $1",
        )
        .bind(topic_id)
        .bind(title)
        .bind(summary)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 关闭 topic：status='closed' + 终版 summary + closed_at=now。
    pub async fn close(&self, topic_id: Uuid, summary: &str) -> Result<()> {
        sqlx::query(
            "UPDATE topic SET status = 'closed', summary = $2, closed_at = $3 WHERE id = $1",
        )
        .bind(topic_id)
        .bind(summary)
        .bind(jiff_sqlx::Timestamp::from(jiff::Timestamp::now()))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 指定 chat 的 active topics（last_active_at 倒序）。
    pub async fn active_by_chat(&self, chat_id: i64) -> Result<Vec<Topic>> {
        let rows = sqlx::query_as::<_, Topic>(
            "SELECT * FROM topic WHERE chat_id = $1 AND status = 'active' \
             ORDER BY last_active_at DESC",
        )
        .bind(chat_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// 时间范围检索（last_active_at 区间；仅 closed——list/search 恒 closed-only）
    pub async fn by_chat_time(
        &self,
        chat_id: i64,
        since: Option<jiff::Timestamp>,
        until: Option<jiff::Timestamp>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Topic>> {
        let mut sql = String::from("SELECT * FROM topic WHERE chat_id = $1 AND status = 'closed'");
        let mut n = 1usize;
        if since.is_some() {
            n += 1;
            sql.push_str(&format!(" AND last_active_at >= ${n}"));
        }
        if until.is_some() {
            n += 1;
            sql.push_str(&format!(" AND last_active_at <= ${n}"));
        }
        n += 1;
        sql.push_str(&format!(" ORDER BY last_active_at DESC LIMIT ${n}"));
        n += 1;
        sql.push_str(&format!(" OFFSET ${n}"));
        let mut q = sqlx::query_as::<_, Topic>(sqlx::AssertSqlSafe(sql.as_str())).bind(chat_id);
        if let Some(s) = since {
            q = q.bind(jiff_sqlx::Timestamp::from(s));
        }
        if let Some(u) = until {
            q = q.bind(jiff_sqlx::Timestamp::from(u));
        }
        let rows = q.bind(limit).bind(offset).fetch_all(&self.pool).await?;
        Ok(rows)
    }

    pub async fn delete_by_id(&self, topic_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM topic WHERE id = $1")
            .bind(topic_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

/// 事务内按该 topic 最早/最晚消息刷新 started_at / last_active_at。
/// `active_at` 语义与 Message::active_at() 一致：sent_at 缺失时回退 created_at。
async fn sync_topic_times(tx: &mut Transaction<'_, Postgres>, topic_id: Uuid) -> Result<()> {
    let first: Option<jiff_sqlx::Timestamp> = sqlx::query_scalar::<_, jiff_sqlx::Timestamp>(
        "SELECT COALESCE(sent_at, created_at) FROM message WHERE topic_id = $1 \
         ORDER BY COALESCE(sent_at, created_at) ASC LIMIT 1",
    )
    .bind(topic_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(ts) = first {
        sqlx::query("UPDATE topic SET started_at = $2 WHERE id = $1")
            .bind(topic_id)
            .bind(ts)
            .execute(&mut **tx)
            .await?;
    }

    let last: Option<jiff_sqlx::Timestamp> = sqlx::query_scalar::<_, jiff_sqlx::Timestamp>(
        "SELECT COALESCE(sent_at, created_at) FROM message WHERE topic_id = $1 \
         ORDER BY COALESCE(sent_at, created_at) DESC LIMIT 1",
    )
    .bind(topic_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(ts) = last {
        sqlx::query("UPDATE topic SET last_active_at = $2 WHERE id = $1")
            .bind(topic_id)
            .bind(ts)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}
