use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    domain::{
        model::{ChatType, ConversationRecord, Platform},
        vo::{ChannelKey, ChapterWrite, ChatEntry, ChatId, ConversationId, EntryKind},
    },
    error::Result,
};

#[derive(Debug, sqlx::FromRow)]
pub struct ConversationRow {
    pub conversation_id: ConversationId,
    pub chat_id: Option<ChatId>,
    pub alias: Option<String>,
    pub external_id: Option<String>,
    pub channel_key: Option<ChannelKey>,
    pub platform: Option<Platform>,
    pub chat_type: Option<ChatType>,
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConversationRecordRepo {
    pool: PgPool,
}

impl ConversationRecordRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn find_by_id(
        &self,
        conversation_id: ConversationId,
    ) -> Result<Option<ConversationRecord>> {
        sqlx::query_as::<_, ConversationRecord>(
            "SELECT id, messages, context_meta, updated_at FROM conversation \
             WHERE id = $1 LIMIT 1",
        )
        .bind(conversation_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    /// Writes the snapshot and consumes the batch in one statement. Consumption is relative to
    /// what `pending` holds when the write lands, which keeps an entry appended while the Turn ran.
    pub async fn save(
        &self,
        conversation_id: ConversationId,
        chapter: ChapterWrite,
        consumed: &[Uuid],
        updated_at: jiff::Timestamp,
    ) -> Result<()> {
        write_snapshot(&self.pool, conversation_id, chapter, consumed, updated_at).await
    }

    pub(crate) async fn save_in_tx(
        tx: &mut sqlx::PgConnection,
        conversation_id: ConversationId,
        chapter: ChapterWrite,
        consumed: &[Uuid],
        updated_at: jiff::Timestamp,
    ) -> Result<()> {
        write_snapshot(&mut *tx, conversation_id, chapter, consumed, updated_at).await
    }

    /// Appends one change on its own transaction, for a producer outside the ingest path.
    pub async fn append_entry(
        &self,
        conversation_id: ConversationId,
        entry: &ChatEntry,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        Self::append_entry_in_tx(&mut tx, conversation_id, entry).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(crate) async fn append_entry_in_tx(
        tx: &mut sqlx::PgConnection,
        conversation_id: ConversationId,
        entry: &ChatEntry,
    ) -> Result<()> {
        let entry = serde_json::to_value(entry)?;
        let updated = sqlx::query(
            "UPDATE conversation \
             SET pending = pending || jsonb_build_array($2::jsonb), updated_at = now() \
             WHERE id = $1",
        )
        .bind(conversation_id)
        .bind(entry)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(crate::error::AppError::message(format!(
                "conversation {conversation_id} not found"
            )));
        }
        Ok(())
    }

    pub async fn pending_entries(&self, conversation_id: ConversationId) -> Result<Vec<ChatEntry>> {
        read_pending(&self.pool, conversation_id).await
    }

    pub(crate) async fn save_pending_in_tx(
        tx: &mut sqlx::PgConnection,
        conversation_id: ConversationId,
        consumed: &[Uuid],
    ) -> Result<()> {
        let updated = sqlx::query(
            "UPDATE conversation SET pending = COALESCE(( \
                 SELECT jsonb_agg(e) FROM jsonb_array_elements(pending) e \
                 WHERE (e->>'id')::uuid <> ALL($2) \
             ), '[]'::jsonb), updated_at = now() WHERE id = $1",
        )
        .bind(conversation_id)
        .bind(consumed)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(crate::error::AppError::message(format!(
                "conversation {conversation_id} not found"
            )));
        }
        Ok(())
    }

    pub(crate) async fn pending_entries_in_tx(
        tx: &mut sqlx::PgConnection,
        conversation_id: ConversationId,
    ) -> Result<Vec<ChatEntry>> {
        read_pending(&mut *tx, conversation_id).await
    }

    /// The Conversations under one local channel that still have pending changes.
    pub async fn conversations_with_pending(
        &self,
        channel_key: &ChannelKey,
    ) -> Result<Vec<ConversationId>> {
        let ids: Vec<i64> = sqlx::query_scalar(
            "SELECT DISTINCT c.conversation_id FROM conversation v \
             JOIN chat c ON c.conversation_id = v.id \
             JOIN platform_channel p ON p.id = c.channel_id \
             WHERE jsonb_array_length(v.pending) > 0 AND p.channel_key = $1 \
             ORDER BY c.conversation_id",
        )
        .bind(channel_key)
        .fetch_all(&self.pool)
        .await?;
        Ok(ids.into_iter().map(ConversationId).collect())
    }

    /// Moves one Chat and its unconsumed entries to another Conversation. History stays in the
    /// source Conversation; only the Chat's address and pending work move.
    pub async fn attach_chat(&self, chat_id: ChatId, target: ConversationId) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        move_chat_in_tx(&mut tx, chat_id, target).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Gives one Chat a fresh Conversation, moving its unconsumed entries with it.
    pub async fn detach_chat(&self, chat_id: ChatId) -> Result<ConversationId> {
        let mut tx = self.pool.begin().await?;
        let target: i64 = sqlx::query_scalar(
            "INSERT INTO conversation (messages, context_meta, pending) \
             VALUES ('[]'::jsonb, \
                     jsonb_build_object('tokens', 0, 'turn_count', 0, 'step_count', 0, \
                                        'session', gen_random_uuid()), \
                     '[]'::jsonb) RETURNING id",
        )
        .fetch_one(&mut *tx)
        .await?;
        let target = ConversationId(target);
        move_chat_in_tx(&mut tx, chat_id, target).await?;
        tx.commit().await?;
        Ok(target)
    }

    pub async fn list(&self) -> Result<Vec<ConversationRow>> {
        sqlx::query_as(
            "SELECT v.id AS conversation_id, c.id AS chat_id, c.alias, c.external_id, p.channel_key, \
                    p.platform, c.chat_type, c.name \
             FROM conversation v LEFT JOIN chat c ON c.conversation_id = v.id \
             LEFT JOIN platform_channel p ON p.id = c.channel_id \
             ORDER BY v.id, c.id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Into::into)
    }
}

async fn move_chat_in_tx(
    tx: &mut sqlx::PgConnection,
    chat_id: ChatId,
    target: ConversationId,
) -> Result<()> {
    let source: Option<ConversationId> =
        sqlx::query_scalar("SELECT conversation_id FROM chat WHERE id = $1 FOR UPDATE")
            .bind(chat_id.0)
            .fetch_optional(&mut *tx)
            .await?;
    let source = source
        .ok_or_else(|| crate::error::AppError::message(format!("Chat {chat_id} not found")))?;
    if source == target {
        return Ok(());
    }
    // Lock both rows in id order: two operators moving Chats in opposite directions must not
    // deadlock on a source/target order that depends on which command ran first.
    let mut ids = [source.0, target.0];
    ids.sort_unstable();
    let locked: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM conversation WHERE id = ANY($1) ORDER BY id FOR UPDATE")
            .bind(ids)
            .fetch_all(&mut *tx)
            .await?;
    if locked.len() != 2 {
        return Err(crate::error::AppError::message(format!(
            "Conversation {target} not found"
        )));
    }

    let source_pending: Option<Value> =
        sqlx::query_scalar("SELECT pending FROM conversation WHERE id = $1")
            .bind(source)
            .fetch_optional(&mut *tx)
            .await?;
    let target_pending: Option<Value> =
        sqlx::query_scalar("SELECT pending FROM conversation WHERE id = $1")
            .bind(target)
            .fetch_optional(&mut *tx)
            .await?;
    let mut source_entries: Vec<ChatEntry> =
        serde_json::from_value(source_pending.unwrap_or_else(|| Value::Array(Vec::new())))?;
    let mut target_entries: Vec<ChatEntry> =
        serde_json::from_value(target_pending.unwrap_or_else(|| Value::Array(Vec::new())))?;
    let (moving, staying): (Vec<_>, Vec<_>) = source_entries.into_iter().partition(|entry| {
        entry.chat_id == chat_id
            && !matches!(
                entry.kind,
                EntryKind::HostAction(_) | EntryKind::PolicyChanged
            )
    });
    source_entries = staying;
    target_entries.extend(moving);

    sqlx::query("UPDATE conversation SET pending = $2, updated_at = now() WHERE id = $1")
        .bind(source)
        .bind(serde_json::to_value(source_entries)?)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE conversation SET pending = $2, updated_at = now() WHERE id = $1")
        .bind(target)
        .bind(serde_json::to_value(target_entries)?)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE chat SET conversation_id = $2, updated_at = now() WHERE id = $1")
        .bind(chat_id.0)
        .bind(target)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

async fn write_snapshot(
    executor: impl sqlx::PgExecutor<'_>,
    conversation_id: ConversationId,
    chapter: ChapterWrite,
    consumed: &[Uuid],
    updated_at: jiff::Timestamp,
) -> Result<()> {
    let updated = sqlx::query(
        "UPDATE conversation SET messages = $2, context_meta = $3, \
         pending = COALESCE((SELECT jsonb_agg(e) FROM jsonb_array_elements(pending) e \
         WHERE (e->>'id')::uuid <> ALL($4)), '[]'::jsonb), updated_at = $5 \
         WHERE id = $1",
    )
    .bind(conversation_id)
    .bind(chapter.messages)
    .bind(chapter.context_meta)
    .bind(consumed)
    .bind(jiff_sqlx::Timestamp::from(updated_at))
    .execute(executor)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(crate::error::AppError::message(format!(
            "conversation {conversation_id} not found"
        )));
    }
    Ok(())
}

async fn read_pending(
    executor: impl sqlx::PgExecutor<'_>,
    conversation_id: ConversationId,
) -> Result<Vec<ChatEntry>> {
    let pending: Option<Value> =
        sqlx::query_scalar("SELECT pending FROM conversation WHERE id = $1")
            .bind(conversation_id)
            .fetch_optional(executor)
            .await?;
    let Some(pending) = pending else {
        return Ok(Vec::new());
    };
    let mut entries: Vec<ChatEntry> = serde_json::from_value(pending)?;
    entries.sort_by(|a, b| a.at.cmp(&b.at).then(a.id.cmp(&b.id)));
    Ok(entries)
}
