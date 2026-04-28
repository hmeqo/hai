use sqlx::PgPool;

use crate::{domain::entity::Identity, error::Result};

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
}
