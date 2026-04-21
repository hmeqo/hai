use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::entity::Account;

pub struct AccountRepo;

impl AccountRepo {
    pub async fn get_or_create(
        pool: &PgPool,
        platform: &str,
        external_id: &str,
        meta: Option<serde_json::Value>,
    ) -> Result<Account> {
        // 1. 先尝试查询，避免直接 INSERT 导致 SERIAL 空洞
        let existing = sqlx::query_as!(
            Account,
            r#"
            SELECT
                id, identity_id as "identity_id: Uuid", platform, external_id, meta,
                last_active_at as "last_active_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            FROM account
            WHERE platform = $1 AND external_id = $2
            "#,
            platform,
            external_id,
        )
        .fetch_optional(pool)
        .await?;

        if let Some(account) = existing {
            // 2. 如果存在，更新 meta 和 last_active_at
            let updated = sqlx::query_as!(
                Account,
                r#"
                UPDATE account
                SET meta = COALESCE($2, meta),
                    last_active_at = CURRENT_TIMESTAMP
                WHERE id = $1
                RETURNING
                    id, identity_id as "identity_id: Uuid", platform, external_id, meta,
                    last_active_at as "last_active_at!: jiff_sqlx::Timestamp",
                    created_at as "created_at!: jiff_sqlx::Timestamp",
                    updated_at as "updated_at!: jiff_sqlx::Timestamp"
                "#,
                account.id,
                meta,
            )
            .fetch_one(pool)
            .await?;
            return Ok(updated);
        }

        // 3. 如果不存在，尝试插入（带上 ON CONFLICT 以防并发竞争）
        let inserted = sqlx::query_as!(
            Account,
            r#"
            INSERT INTO account (platform, external_id, meta)
            VALUES ($1, $2, $3)
            ON CONFLICT (platform, external_id) DO UPDATE
            SET meta = COALESCE(EXCLUDED.meta, account.meta),
                last_active_at = CURRENT_TIMESTAMP
            RETURNING
                id, identity_id as "identity_id: Uuid", platform, external_id, meta,
                last_active_at as "last_active_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            "#,
            platform,
            external_id,
            meta,
        )
        .fetch_one(pool)
        .await?;
        Ok(inserted)
    }

    pub async fn find_by_id(pool: &PgPool, id: i64) -> Result<Option<Account>> {
        let account = sqlx::query_as!(
            Account,
            r#"
            SELECT
                id,
                identity_id as "identity_id: Uuid",
                platform,
                external_id,
                meta,
                last_active_at as "last_active_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            FROM account
            WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(pool)
        .await?;
        Ok(account)
    }

    pub async fn list_by_identity_id(pool: &PgPool, identity_id: Uuid) -> Result<Vec<Account>> {
        let accounts = sqlx::query_as!(
            Account,
            r#"
            SELECT
                id,
                identity_id as "identity_id: Uuid",
                platform,
                external_id,
                meta,
                last_active_at as "last_active_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            FROM account
            WHERE identity_id = $1
            "#,
            identity_id,
        )
        .fetch_all(pool)
        .await?;
        Ok(accounts)
    }

    pub async fn bind_identity(pool: &PgPool, id: i64, identity_id: Uuid) -> Result<u64> {
        let result = sqlx::query!(
            "UPDATE account SET identity_id = $1 WHERE id = $2",
            identity_id,
            id,
        )
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }
}
