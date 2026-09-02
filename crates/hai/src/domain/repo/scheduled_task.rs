use sqlx::PgPool;
use uuid::Uuid;

use crate::{domain::model::ScheduledTask, error::Result};

#[derive(Debug, Clone)]
pub struct ScheduledTaskRepo {
    pool: PgPool,
}

impl ScheduledTaskRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        bot_id: &str,
        chat_id: i64,
        description: &str,
        fire_at: jiff::Timestamp,
        every_secs: Option<i64>,
    ) -> Result<ScheduledTask> {
        let id = Uuid::now_v7();
        sqlx::query_as::<_, ScheduledTask>(
            "INSERT INTO scheduled_task (id, bot_id, chat_id, description, fire_at, every_secs) \
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING *",
        )
        .bind(id)
        .bind(bot_id)
        .bind(chat_id)
        .bind(description)
        .bind(jiff_sqlx::Timestamp::from(fire_at))
        .bind(every_secs)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn list_active_by_chat(
        &self,
        bot_id: &str,
        chat_id: i64,
    ) -> Result<Vec<ScheduledTask>> {
        sqlx::query_as::<_, ScheduledTask>(
            "SELECT * FROM scheduled_task \
             WHERE bot_id = $1 AND chat_id = $2 AND is_active \
             ORDER BY fire_at",
        )
        .bind(bot_id)
        .bind(chat_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn list_all_by_chat(&self, bot_id: &str, chat_id: i64) -> Result<Vec<ScheduledTask>> {
        sqlx::query_as::<_, ScheduledTask>(
            "SELECT * FROM scheduled_task \
             WHERE bot_id = $1 AND chat_id = $2 \
             ORDER BY fire_at",
        )
        .bind(bot_id)
        .bind(chat_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn deactivate(&self, bot_id: &str, chat_id: i64, id: Uuid) -> Result<()> {
        sqlx::query(
            "UPDATE scheduled_task SET is_active = FALSE, updated_at = now() \
             WHERE id = $1 AND bot_id = $2 AND chat_id = $3",
        )
        .bind(id)
        .bind(bot_id)
        .bind(chat_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 到期任务：某 bot 下 fire_at <= now 的激活任务。
    pub async fn due(&self, bot_id: &str, now: jiff::Timestamp) -> Result<Vec<ScheduledTask>> {
        sqlx::query_as::<_, ScheduledTask>(
            "SELECT * FROM scheduled_task \
             WHERE bot_id = $1 AND is_active AND fire_at <= $2 \
             ORDER BY fire_at",
        )
        .bind(bot_id)
        .bind(jiff_sqlx::Timestamp::from(now))
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    /// 推进周期任务 fire_at 到将来首个触发点；一次性任务 deactivate。
    pub async fn advance_fire_at(
        &self,
        id: Uuid,
        next_fire: Option<jiff::Timestamp>,
    ) -> Result<()> {
        let sql = if next_fire.is_some() {
            "UPDATE scheduled_task SET fire_at = $2, updated_at = now() WHERE id = $1"
        } else {
            "UPDATE scheduled_task SET is_active = FALSE, updated_at = now() WHERE id = $1"
        };
        let mut q = sqlx::query(sql).bind(id);
        if let Some(next) = next_fire {
            q = q.bind(jiff_sqlx::Timestamp::from(next));
        }
        q.execute(&self.pool).await?;
        Ok(())
    }
}
