//! One purge's transaction: every member Chat's agent surface leaves together and the conversation
//! row is rewritten to the chapter it continues from.

use std::collections::HashSet;

use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    domain::{
        repo::{
            ConversationRecordRepo, MemoryRepo, MessageRepo, ScheduledTaskRepo, TopicRepo,
            message::DeletedMessage,
        },
        vo::{
            ChapterWrite, ChatEntry, ChatId, ConversationId, EntryKind, MessageId, PurgeCounts,
            PurgeSurfaces, ScheduledTaskId, TimeRange, TopicId,
        },
    },
    error::Result,
};

#[derive(Debug, Clone)]
pub struct PurgeRepo {
    pool: PgPool,
}

/// What one purge's transaction removed, and the changes it took with it.
#[derive(Debug)]
pub struct PurgeWiped {
    pub counts: PurgeCounts,
    pub consumed: Vec<Uuid>,
}

impl PurgeRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Wipes a Conversation's agent surface in one transaction. Every member Chat's messages,
    /// memories and scheduled tasks are in scope; the conversation row is always rewritten.
    pub async fn wipe(
        &self,
        conversation_id: ConversationId,
        surfaces: PurgeSurfaces,
        range: TimeRange,
        chapter: ChapterWrite,
        also_consumed: &[Uuid],
    ) -> Result<PurgeWiped> {
        let mut tx = self.pool.begin().await?;
        let chat_ids: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM chat WHERE conversation_id = $1 ORDER BY id")
                .bind(conversation_id)
                .fetch_all(&mut *tx)
                .await?;

        let pending =
            ConversationRecordRepo::pending_entries_in_tx(&mut tx, conversation_id).await?;
        let mut counts = PurgeCounts::default();
        let mut consumed = also_consumed.to_vec();

        if surfaces.agent {
            let mut deleted_messages: Vec<DeletedMessage> = Vec::new();
            let mut deleted_tasks: Vec<ScheduledTaskId> = Vec::new();
            for chat_id in chat_ids {
                let deleted = MessageRepo::delete_in_range_in_tx(
                    &mut tx,
                    conversation_id,
                    ChatId(chat_id),
                    range,
                )
                .await?;
                counts.messages += deleted.len() as u64;
                deleted_messages.extend(deleted);

                let tasks =
                    ScheduledTaskRepo::delete_in_range_in_tx(&mut tx, ChatId(chat_id), range)
                        .await?;
                counts.scheduled += tasks.len() as u64;
                deleted_tasks.extend(tasks);
            }

            let topics = topics_of(&deleted_messages);
            counts.topics =
                TopicRepo::delete_many_in_tx(&mut tx, conversation_id, &topics, range).await?;

            let message_ids: Vec<MessageId> =
                deleted_messages.iter().map(|message| message.id).collect();
            counts.memories =
                MemoryRepo::delete_in_range_in_tx(&mut tx, conversation_id, &message_ids, range)
                    .await?;

            consumed.extend(invalidated(&pending, &deleted_messages, &deleted_tasks));
        }
        consumed.sort_unstable();
        consumed.dedup();

        if surfaces.agent {
            ConversationRecordRepo::save_in_tx(
                &mut tx,
                conversation_id,
                chapter,
                &consumed,
                jiff::Timestamp::now(),
            )
            .await?;
        } else {
            ConversationRecordRepo::save_pending_in_tx(&mut tx, conversation_id, &consumed).await?;
        }
        tx.commit().await?;

        Ok(PurgeWiped { counts, consumed })
    }
}

fn topics_of(deleted: &[DeletedMessage]) -> Vec<TopicId> {
    let mut topics: Vec<TopicId> = deleted
        .iter()
        .filter_map(|message| message.topic_id)
        .collect();
    topics.sort_unstable();
    topics.dedup();
    topics
}

fn invalidated(
    pending: &[ChatEntry],
    deleted: &[DeletedMessage],
    tasks: &[ScheduledTaskId],
) -> Vec<Uuid> {
    let message_ids: HashSet<MessageId> = deleted.iter().map(|message| message.id).collect();
    let task_ids: HashSet<ScheduledTaskId> = tasks.iter().copied().collect();
    pending
        .iter()
        .filter(|entry| match &entry.kind {
            EntryKind::MessageCreated { message_id }
            | EntryKind::MessageEdited { message_id, .. } => message_ids.contains(message_id),
            EntryKind::Scheduled { task_id, .. } => {
                task_id.is_some_and(|id| task_ids.contains(&id))
            }
            EntryKind::Command(_)
            | EntryKind::PolicyChanged
            | EntryKind::HostAction(_)
            | EntryKind::Purge(_) => false,
        })
        .map(|entry| entry.id)
        .collect()
}
