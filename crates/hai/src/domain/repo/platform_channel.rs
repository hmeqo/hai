use sqlx::PgPool;

use crate::{
    domain::{
        model::{Platform, PlatformChannel},
        vo::{ChannelKey, ChatOrigin, PlatformChannelId},
    },
    error::{AppError, Result},
};

#[derive(Debug, Clone)]
pub struct PlatformChannelRepo {
    pool: PgPool,
}

impl PlatformChannelRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn by_key(&self, key: &ChannelKey) -> Result<Option<PlatformChannel>> {
        sqlx::query_as::<_, PlatformChannel>(
            "SELECT * FROM platform_channel WHERE channel_key = $1",
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn by_id(&self, id: PlatformChannelId) -> Result<Option<PlatformChannel>> {
        sqlx::query_as::<_, PlatformChannel>("SELECT * FROM platform_channel WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
    }

    pub async fn create(
        &self,
        key: &ChannelKey,
        platform: Platform,
        origin: Option<&ChatOrigin>,
    ) -> Result<PlatformChannel> {
        sqlx::query_as::<_, PlatformChannel>(
            "INSERT INTO platform_channel (channel_key, platform, origin) \
             VALUES ($1, $2, $3) RETURNING *",
        )
        .bind(key)
        .bind(platform.as_str())
        .bind(origin)
        .fetch_one(&self.pool)
        .await
        .map_err(Into::into)
    }

    /// Sets the origin exactly once. A caller that found the row with a null origin can use this
    /// without racing another writer into changing it.
    pub async fn set_origin_if_null(
        &self,
        id: PlatformChannelId,
        origin: &ChatOrigin,
    ) -> Result<PlatformChannel> {
        let channel = sqlx::query_as::<_, PlatformChannel>(
            "UPDATE platform_channel SET origin = $2, updated_at = now() \
             WHERE id = $1 AND origin IS NULL RETURNING *",
        )
        .bind(id)
        .bind(origin)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(channel) = channel {
            return Ok(channel);
        }
        let current = self
            .by_id(id)
            .await?
            .ok_or_else(|| AppError::message(format!("platform channel {id} disappeared")))?;
        match current.origin.as_ref() {
            Some(stored) if stored == origin => Ok(current),
            _ => Err(AppError::message(format!(
                "platform channel {id}: origin was set by another writer"
            ))),
        }
    }
}
