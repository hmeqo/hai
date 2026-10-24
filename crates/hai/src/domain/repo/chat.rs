use sqlx::PgPool;

use crate::{
    config::conversation::ConversationScope,
    domain::{
        model::{Chat, NewChat},
        vo::{ChatId, ContextMeta, ConversationId, PlatformChannelId},
    },
    error::{AppError, Result},
};

#[derive(Debug, Clone)]
pub struct ChatRepo {
    pool: PgPool,
}

impl ChatRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn by_external_id(
        &self,
        channel_id: PlatformChannelId,
        external_id: &str,
    ) -> Result<Option<Chat>> {
        sqlx::query_as::<_, Chat>(
            "SELECT c.*, p.platform FROM chat c \
             JOIN platform_channel p ON p.id = c.channel_id \
             WHERE c.channel_id = $1 AND c.external_id = $2 LIMIT 1",
        )
        .bind(channel_id)
        .bind(external_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    /// Inserts the Chat and, when the scope calls for it, creates or reuses the channel's shared
    /// Conversation in the same transaction. The scope is the channel's only rule; a private Chat is
    /// not a special case.
    pub async fn create(&self, new: NewChat, scope: ConversationScope) -> Result<Chat> {
        let mut tx = self.pool.begin().await?;
        let default: Option<Option<i64>> = sqlx::query_scalar(
            "SELECT default_conversation_id FROM platform_channel WHERE id = $1 FOR UPDATE",
        )
        .bind(new.channel_id)
        .fetch_optional(&mut *tx)
        .await?;
        let default = default.ok_or_else(|| {
            AppError::message(format!("platform channel {} not found", new.channel_id))
        })?;

        let shared = scope == ConversationScope::Shared;
        let conversation_id = match (shared, default) {
            (true, Some(id)) => ConversationId(id),
            _ => {
                let id: i64 = sqlx::query_scalar(
                    "INSERT INTO conversation (messages, context_meta, pending) \
                     VALUES ('[]'::jsonb, $1, '[]'::jsonb) RETURNING id",
                )
                .bind(serde_json::to_value(ContextMeta::new())?)
                .fetch_one(&mut *tx)
                .await?;
                let id = ConversationId(id);
                if shared {
                    sqlx::query(
                        "UPDATE platform_channel SET default_conversation_id = $2, updated_at = now() \
                         WHERE id = $1",
                    )
                    .bind(new.channel_id)
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
                }
                id
            }
        };

        let chat = sqlx::query_as::<_, Chat>(
            "WITH next AS ( \
                 SELECT nextval(pg_get_serial_sequence('chat', 'id')) AS id \
             ), inserted AS ( \
                 INSERT INTO chat \
                 (id, channel_id, conversation_id, alias, external_id, chat_type, name, created_at, updated_at) \
                 SELECT id, $1, $2, 'chat-' || id, $3, $4, $5, $6, $7 FROM next \
                 RETURNING * \
             ) \
             SELECT i.*, p.platform FROM inserted i \
             JOIN platform_channel p ON p.id = i.channel_id",
        )
        .bind(new.channel_id)
        .bind(conversation_id)
        .bind(new.external_id)
        .bind(new.chat_type.as_str())
        .bind(new.name)
        .bind(jiff_sqlx::Timestamp::from(new.created_at))
        .bind(jiff_sqlx::Timestamp::from(new.updated_at))
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(chat)
    }

    pub async fn by_id(&self, id: ChatId) -> Result<Option<Chat>> {
        sqlx::query_as::<_, Chat>(
            "SELECT c.*, p.platform FROM chat c \
             JOIN platform_channel p ON p.id = c.channel_id WHERE c.id = $1",
        )
        .bind(id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    pub async fn by_conversation(&self, conversation_id: ConversationId) -> Result<Vec<Chat>> {
        sqlx::query_as::<_, Chat>(
            "SELECT c.*, p.platform FROM chat c \
             JOIN platform_channel p ON p.id = c.channel_id \
             WHERE c.conversation_id = $1 ORDER BY c.id",
        )
        .bind(conversation_id)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }

    /// Chats by id, including ones no longer in the caller's Conversation, for history attribution.
    pub async fn by_ids(&self, ids: &[ChatId]) -> Result<Vec<Chat>> {
        sqlx::query_as::<_, Chat>(
            "SELECT c.*, p.platform FROM chat c \
             JOIN platform_channel p ON p.id = c.channel_id \
             WHERE c.id = ANY($1) ORDER BY c.id",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::domain::model::ChatType;

    /// A disposable database is opt-in: the default suite runs without PostgreSQL.
    #[tokio::test]
    async fn shared_scope_reuses_one_conversation_and_isolated_does_not() {
        let Ok(url) = std::env::var("HAI_TEST_DATABASE_URL") else {
            return;
        };
        let pool = PgPool::connect(&url).await.expect("test database");
        let repo = ChatRepo::new(pool.clone());
        let key = format!("test:{}", Uuid::new_v4());
        let channel: i64 = sqlx::query_scalar(
            "INSERT INTO platform_channel (channel_key, platform) \
             VALUES ($1, 'telegram') RETURNING id",
        )
        .bind(&key)
        .fetch_one(&pool)
        .await
        .expect("test channel");
        let now = jiff::Timestamp::now();
        let new = |external: &str, chat_type| NewChat {
            channel_id: PlatformChannelId(channel),
            external_id: external.to_owned(),
            chat_type,
            name: None,
            created_at: now,
            updated_at: now,
        };

        let group_a = repo
            .create(new("a", ChatType::Group), ConversationScope::Shared)
            .await
            .expect("first shared chat");
        let group_b = repo
            .create(new("b", ChatType::Group), ConversationScope::Shared)
            .await
            .expect("second shared chat");
        let private = repo
            .create(new("p", ChatType::Private), ConversationScope::Shared)
            .await
            .expect("private chat");
        assert_eq!(group_a.conversation_id, group_b.conversation_id);
        assert_eq!(group_a.conversation_id, private.conversation_id);

        let isolated = repo
            .create(new("i", ChatType::Group), ConversationScope::Isolated)
            .await
            .expect("isolated chat");
        assert_ne!(isolated.conversation_id, group_a.conversation_id);

        let found = repo
            .by_ids(&[group_a.id, private.id])
            .await
            .expect("chats by id");
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|chat| chat.id == group_a.id));
        assert!(found.iter().any(|chat| chat.id == private.id));

        sqlx::query("DELETE FROM chat WHERE channel_id = $1")
            .bind(channel)
            .execute(&pool)
            .await
            .expect("cleanup chats");
        sqlx::query("DELETE FROM platform_channel WHERE id = $1")
            .bind(channel)
            .execute(&pool)
            .await
            .expect("cleanup channel");
        sqlx::query("DELETE FROM conversation WHERE id = ANY($1)")
            .bind([group_a.conversation_id.0, isolated.conversation_id.0])
            .execute(&pool)
            .await
            .expect("cleanup conversations");
    }
}
