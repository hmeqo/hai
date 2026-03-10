use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::app::domain::entity::Identity;

pub struct IdentityRepo;

impl IdentityRepo {
    pub async fn create(
        pool: &PgPool,
        name: Option<&str>,
        meta: Option<serde_json::Value>,
    ) -> Result<Identity> {
        let identity = sqlx::query_as!(
            Identity,
            r#"
            INSERT INTO identity (name, meta)
            VALUES ($1, $2)
            RETURNING
                id,
                name,
                meta,
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            "#,
            name,
            meta,
        )
        .fetch_one(pool)
        .await?;
        Ok(identity)
    }

    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Identity>> {
        let identity = sqlx::query_as!(
            Identity,
            r#"
            SELECT
                id,
                name,
                meta,
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            FROM identity
            WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(pool)
        .await?;
        Ok(identity)
    }

    pub async fn update(
        pool: &PgPool,
        id: Uuid,
        name: Option<&str>,
        meta: Option<serde_json::Value>,
    ) -> Result<Option<Identity>> {
        let identity = sqlx::query_as!(
            Identity,
            r#"
            UPDATE identity
            SET name = COALESCE($1, name),
                meta = COALESCE($2, meta)
            WHERE id = $3
            RETURNING
                id,
                name,
                meta,
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            "#,
            name,
            meta,
            id,
        )
        .fetch_optional(pool)
        .await?;
        Ok(identity)
    }
}
