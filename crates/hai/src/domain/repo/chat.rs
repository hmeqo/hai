use anyhow::Result;
use sqlx::PgPool;

use crate::domain::entity::Chat;

pub struct ChatRepo;

impl ChatRepo {
    /// 通过平台标识获取或创建会话/群组，返回内部会话记录
    pub async fn get_or_create(
        pool: &PgPool,
        platform: &str,
        external_id: &str,
        chat_type: &str,
        name: Option<&str>,
        meta: Option<serde_json::Value>,
    ) -> Result<Chat> {
        // 1. 先尝试查询，避免直接 INSERT 导致 SERIAL 空洞
        let existing = sqlx::query_as!(
            Chat,
            r#"
            SELECT
                id, platform, external_id, chat_type, name, config, meta,
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            FROM chat
            WHERE platform = $1 AND external_id = $2
            "#,
            platform,
            external_id,
        )
        .fetch_optional(pool)
        .await?;

        if let Some(chat) = existing {
            // 2. 如果存在，更新 name 和 meta
            let updated = sqlx::query_as!(
                Chat,
                r#"
                UPDATE chat
                SET name = COALESCE($2, name),
                    meta = COALESCE($3, meta)
                WHERE id = $1
                RETURNING
                    id, platform, external_id, chat_type, name, config, meta,
                    created_at as "created_at!: jiff_sqlx::Timestamp",
                    updated_at as "updated_at!: jiff_sqlx::Timestamp"
                "#,
                chat.id,
                name,
                meta,
            )
            .fetch_one(pool)
            .await?;
            return Ok(updated);
        }

        // 3. 如果不存在，尝试插入（带上 ON CONFLICT 以防并发竞争）
        let inserted = sqlx::query_as!(
            Chat,
            r#"
            INSERT INTO chat (platform, external_id, chat_type, name, meta)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (platform, external_id) DO UPDATE
            SET name = COALESCE(EXCLUDED.name, chat.name),
                meta = COALESCE(EXCLUDED.meta, chat.meta)
            RETURNING
                id, platform, external_id, chat_type, name, config, meta,
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            "#,
            platform,
            external_id,
            chat_type,
            name,
            meta,
        )
        .fetch_one(pool)
        .await?;
        Ok(inserted)
    }

    /// 通过内部 ID 查询会话
    pub async fn find_by_id(pool: &PgPool, id: i64) -> Result<Option<Chat>> {
        let chat = sqlx::query_as!(
            Chat,
            r#"
            SELECT
                id,
                platform,
                external_id,
                chat_type,
                name,
                config,
                meta,
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            FROM chat
            WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(pool)
        .await?;
        Ok(chat)
    }
}
