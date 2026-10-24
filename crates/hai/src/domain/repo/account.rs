use serde_json::Value;
use sqlx::PgPool;

use crate::{
    domain::{
        model::Account,
        vo::{AccountId, PlatformChannelId},
    },
    error::Result,
};

#[derive(Debug, Clone)]
pub struct AccountRepo {
    pool: PgPool,
}

impl AccountRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn by_channel_external_id(
        &self,
        channel_id: PlatformChannelId,
        external_id: &str,
    ) -> Result<Option<Account>> {
        sqlx::query_as::<_, Account>(
            "SELECT * FROM account WHERE channel_id = $1 AND external_id = $2 LIMIT 1",
        )
        .bind(channel_id)
        .bind(external_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn create(
        &self,
        channel_id: PlatformChannelId,
        external_id: &str,
        meta: Option<Value>,
        last_active_at: jiff::Timestamp,
        created_at: jiff::Timestamp,
        updated_at: jiff::Timestamp,
    ) -> Result<Account> {
        sqlx::query_as::<_, Account>(
            "INSERT INTO account (channel_id, external_id, meta, last_active_at, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING *",
        )
        .bind(channel_id)
        .bind(external_id)
        .bind(meta)
        .bind(jiff_sqlx::Timestamp::from(last_active_at))
        .bind(jiff_sqlx::Timestamp::from(created_at))
        .bind(jiff_sqlx::Timestamp::from(updated_at))
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn update_last_active_at(&self, id: AccountId, at: jiff::Timestamp) -> Result<()> {
        sqlx::query("UPDATE account SET last_active_at = $2 WHERE id = $1")
            .bind(id.0)
            .bind(jiff_sqlx::Timestamp::from(at))
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn by_id(&self, id: AccountId) -> Result<Option<Account>> {
        sqlx::query_as::<_, Account>("SELECT * FROM account WHERE id = $1")
            .bind(id.0)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }
}
