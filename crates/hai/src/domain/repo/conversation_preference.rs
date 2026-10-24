use serde_json::Value;
use sqlx::PgPool;

use crate::{
    domain::{
        model::ConversationPreference,
        vo::{ConversationId, PreferenceKey},
    },
    error::Result,
};

#[derive(Debug, Clone)]
pub struct ConversationPreferenceRepo {
    pool: PgPool,
}

impl ConversationPreferenceRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get(
        &self,
        conversation_id: ConversationId,
    ) -> Result<Option<ConversationPreference>> {
        sqlx::query_as::<_, ConversationPreference>(
            "SELECT * FROM conversation_preference WHERE conversation_id = $1",
        )
        .bind(conversation_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn set_key(
        &self,
        conversation_id: ConversationId,
        key: PreferenceKey,
        value: &Value,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO conversation_preference (conversation_id, prefs, updated_at) \
             VALUES ($1, jsonb_build_object($2, $3), now()) \
             ON CONFLICT (conversation_id) DO UPDATE \
             SET prefs = conversation_preference.prefs || EXCLUDED.prefs, updated_at = now()",
        )
        .bind(conversation_id)
        .bind(key.as_str())
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn remove_key(
        &self,
        conversation_id: ConversationId,
        key: PreferenceKey,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE conversation_preference SET prefs = prefs - $2, updated_at = now() \
             WHERE conversation_id = $1",
        )
        .bind(conversation_id)
        .bind(key.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
