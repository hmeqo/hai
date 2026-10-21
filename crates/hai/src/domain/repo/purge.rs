//! One purge's transaction: the agent surface's rows leave together and the conversation row is
//! rewritten to the chapter the chat continues from. The changes the deletes invalidate are consumed
//! by the same statement, so a landed purge cannot be read back as a backlog.

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
            ChapterWrite, ChatEntry, ChatId, EntryKind, MessageId, PurgeCounts, PurgeSurfaces,
            ScheduledTaskId, TimeRange, TopicId,
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
    /// The changes that left the accumulation: the ones whose message or task was deleted, plus the
    /// ones the caller named.
    pub consumed: Vec<Uuid>,
}

impl PurgeRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Wipes one chat's agent surface in a single transaction. `chapter` is the snapshot the
    /// conversation row is rewritten to, always written: a purge that touched no row of the agent
    /// surface still has to consume the change that asked for it, or every tick would ask again.
    pub async fn wipe(
        &self,
        chat_id: ChatId,
        surfaces: PurgeSurfaces,
        range: TimeRange,
        chapter: ChapterWrite,
        also_consumed: &[Uuid],
    ) -> Result<PurgeWiped> {
        let mut tx = self.pool.begin().await?;

        let pending = ConversationRecordRepo::pending_entries_in_tx(&mut tx, chat_id).await?;
        let mut counts = PurgeCounts::default();
        let mut consumed = also_consumed.to_vec();

        if surfaces.agent {
            let deleted = MessageRepo::delete_in_range_in_tx(&mut tx, chat_id, range).await?;
            counts.messages = deleted.len() as u64;

            let topics = topics_of(&deleted);
            counts.topics = TopicRepo::delete_many_in_tx(&mut tx, chat_id, &topics, range).await?;

            let message_ids: Vec<MessageId> = deleted.iter().map(|message| message.id).collect();
            counts.memories =
                MemoryRepo::delete_in_range_in_tx(&mut tx, chat_id, &message_ids, range).await?;

            let tasks = ScheduledTaskRepo::delete_in_range_in_tx(&mut tx, chat_id, range).await?;
            counts.scheduled = tasks.len() as u64;

            consumed.extend(invalidated(&pending, &deleted, &tasks));
        }
        consumed.sort_unstable();
        consumed.dedup();

        ConversationRecordRepo::save_in_tx(
            &mut tx,
            chat_id,
            chapter,
            &consumed,
            jiff::Timestamp::now(),
        )
        .await?;
        tx.commit().await?;

        Ok(PurgeWiped { counts, consumed })
    }
}

/// The topics the deleted messages were filed under, each once.
fn topics_of(deleted: &[DeletedMessage]) -> Vec<TopicId> {
    let mut topics: Vec<TopicId> = deleted
        .iter()
        .filter_map(|message| message.topic_id)
        .collect();
    topics.sort_unstable();
    topics.dedup();
    topics
}

/// The changes a purge invalidated: the ones whose message or task the transaction deleted. A change
/// about a row that is gone has nothing left to read.
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
            EntryKind::Command(_) | EntryKind::Purge(_) => false,
        })
        .map(|entry| entry.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: EntryKind) -> ChatEntry {
        ChatEntry::new(
            crate::domain::vo::EntryAudience::Agent,
            kind,
            jiff::Timestamp::from_second(1_700_000_000).expect("a valid second"),
        )
    }

    /// A change is consumed by what it is about, never by arriving in the same batch: an instruction
    /// that has not run yet stays owed.
    #[test]
    fn only_the_changes_whose_row_went_are_invalidated() {
        let gone = DeletedMessage {
            id: MessageId(7),
            topic_id: None,
        };
        let task = ScheduledTaskId(Uuid::from_u128(3));
        let batch = vec![
            entry(EntryKind::MessageCreated {
                message_id: MessageId(7),
            }),
            entry(EntryKind::MessageEdited {
                message_id: MessageId(8),
                edited_at: jiff::Timestamp::from_second(1_700_000_000).expect("a valid second"),
            }),
            entry(EntryKind::Scheduled {
                task_id: Some(task),
                description: "take the medication".to_owned(),
            }),
            entry(EntryKind::Command(
                crate::domain::vo::AgentCommand::OrganizeMemory,
            )),
        ];

        let consumed = invalidated(&batch, &[gone], &[task]);

        assert_eq!(consumed, vec![batch[0].id, batch[2].id]);
    }

    /// The topics a message was filed under are deleted once each, whatever the window held.
    #[test]
    fn topics_are_collected_once() {
        let message = |topic: Option<TopicId>| DeletedMessage {
            id: MessageId(1),
            topic_id: topic,
        };
        let first = TopicId(Uuid::from_u128(1));
        let second = TopicId(Uuid::from_u128(2));

        assert_eq!(
            topics_of(&[
                message(Some(first)),
                message(None),
                message(Some(first)),
                message(Some(second)),
            ]),
            vec![first, second]
        );
    }
}
