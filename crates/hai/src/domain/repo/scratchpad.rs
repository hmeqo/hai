use sqlx::PgPool;

use crate::{domain::model::Scratchpad, error::Result};

#[derive(Debug, Clone)]
pub struct ScratchpadRepo {
    pool: PgPool,
}

impl ScratchpadRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn find_by_chat_id(&self, chat_id: i64) -> Result<Option<Scratchpad>> {
        sqlx::query_as::<_, Scratchpad>("SELECT * FROM scratchpad WHERE chat_id = $1")
            .bind(chat_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn create(
        &self,
        chat_id: i64,
        content: &str,
        token_count: i32,
        updated_at: jiff::Timestamp,
    ) -> Result<Scratchpad> {
        sqlx::query_as::<_, Scratchpad>(
            "INSERT INTO scratchpad (chat_id, content, token_count, updated_at) \
             VALUES ($1, $2, $3, $4) RETURNING *",
        )
        .bind(chat_id)
        .bind(content)
        .bind(token_count)
        .bind(jiff_sqlx::Timestamp::from(updated_at))
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn update_content(&self, chat_id: i64, content: &str) -> Result<Scratchpad> {
        sqlx::query_as::<_, Scratchpad>(
            "UPDATE scratchpad SET content = $2 WHERE chat_id = $1 RETURNING *",
        )
        .bind(chat_id)
        .bind(content)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }
}
