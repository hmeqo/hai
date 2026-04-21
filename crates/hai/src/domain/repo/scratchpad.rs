use anyhow::Result;
use sqlx::PgPool;

use crate::domain::entity::Scratchpad;

pub struct ScratchpadRepo;

impl ScratchpadRepo {
    /// 获取 chat 的 scratchpad，不存在则返回 None
    pub async fn find(pool: &PgPool, chat_id: i64) -> Result<Option<Scratchpad>> {
        let row = sqlx::query_as!(
            Scratchpad,
            r#"
            SELECT
                chat_id,
                content,
                token_count,
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            FROM scratchpad
            WHERE chat_id = $1
            "#,
            chat_id,
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// 写入或更新 scratchpad 内容
    pub async fn upsert(
        pool: &PgPool,
        chat_id: i64,
        content: &str,
        token_count: i32,
    ) -> Result<Scratchpad> {
        let row = sqlx::query_as!(
            Scratchpad,
            r#"
            INSERT INTO scratchpad (chat_id, content, token_count)
            VALUES ($1, $2, $3)
            ON CONFLICT (chat_id) DO UPDATE
            SET content     = EXCLUDED.content,
                token_count = EXCLUDED.token_count,
                updated_at  = NOW()
            RETURNING
                chat_id,
                content,
                token_count,
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            "#,
            chat_id,
            content,
            token_count,
        )
        .fetch_one(pool)
        .await?;
        Ok(row)
    }
}
