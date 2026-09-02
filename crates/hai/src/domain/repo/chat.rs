use serde_json::Value;
use sqlx::PgPool;

use crate::{domain::model::Chat, error::Result};

#[derive(Debug, Clone)]
pub struct ChatRepo {
    pool: PgPool,
}

impl ChatRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn by_platform_external_id(
        &self,
        platform: &str,
        external_id: &str,
    ) -> Result<Option<Chat>> {
        sqlx::query_as::<_, Chat>(
            "SELECT * FROM chat WHERE platform = $1 AND external_id = $2 LIMIT 1",
        )
        .bind(platform)
        .bind(external_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn create(
        &self,
        platform: &str,
        external_id: &str,
        chat_type: &str,
        name: Option<&str>,
        meta: Option<Value>,
        created_at: jiff::Timestamp,
        updated_at: jiff::Timestamp,
    ) -> Result<Chat> {
        sqlx::query_as::<_, Chat>(
            "INSERT INTO chat (platform, external_id, chat_type, name, meta, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING *",
        )
        .bind(platform)
        .bind(external_id)
        .bind(chat_type)
        .bind(name)
        .bind(meta)
        .bind(jiff_sqlx::Timestamp::from(created_at))
        .bind(jiff_sqlx::Timestamp::from(updated_at))
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn by_id(&self, id: i64) -> Result<Option<Chat>> {
        sqlx::query_as::<_, Chat>("SELECT * FROM chat WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }
}
