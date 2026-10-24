use uuid::Uuid;

use crate::{
    domain::{
        model::ConversationRecord,
        repo::Repos,
        vo::{
            ChannelKey, ChapterWrite, ChatEntry, ChatId, ContextMeta, ConversationId,
            ConversationSnapshot, EntryAudience, EntryKind, HostAction,
        },
    },
    error::{AppError, Result},
};

#[derive(Debug)]
pub struct ConversationRecordService {
    repos: Repos,
}

impl ConversationRecordService {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }

    /// `Ok(None)` means there is no record yet; a database failure surfaces as `Err` instead of
    /// being turned into one.
    pub async fn get(&self, conversation_id: ConversationId) -> Result<Option<ConversationRecord>> {
        self.repos
            .conversation_record
            .find_by_id(conversation_id)
            .await
    }

    pub(crate) async fn save(
        &self,
        snap: &ConversationSnapshot,
        conversation_id: ConversationId,
        consumed: &[Uuid],
    ) -> Result<()> {
        self.repos
            .conversation_record
            .save(
                conversation_id,
                snapshot_values(snap)?,
                consumed,
                jiff::Timestamp::now(),
            )
            .await
    }

    /// The clean chapter a purge resets a conversation to.
    pub(crate) fn fresh_chapter(&self) -> Result<ChapterWrite> {
        snapshot_values(&ConversationSnapshot {
            context_messages: Vec::new(),
            context_meta: ContextMeta::new(),
        })
    }

    pub(crate) fn restore(&self, record: &ConversationRecord) -> Result<ConversationSnapshot> {
        restore_snapshot(record)
    }

    pub async fn append_entry(
        &self,
        conversation_id: ConversationId,
        entry: &ChatEntry,
    ) -> Result<()> {
        self.repos
            .conversation_record
            .append_entry(conversation_id, entry)
            .await
    }

    /// Records a setting the host changed on its own. The entry is context for the next Turn and no
    /// reason to start one, so nothing is rung here. An append failure is returned: a change the
    /// agent will never see is not a success.
    pub async fn record_host_action(
        &self,
        conversation_id: ConversationId,
        chat_id: ChatId,
        action: HostAction,
    ) -> Result<()> {
        let entry = ChatEntry::new(
            chat_id,
            EntryAudience::Context,
            EntryKind::HostAction(action),
            jiff::Timestamp::now(),
        );
        self.append_entry(conversation_id, &entry).await
    }

    pub async fn pending_entries(&self, conversation_id: ConversationId) -> Result<Vec<ChatEntry>> {
        self.repos
            .conversation_record
            .pending_entries(conversation_id)
            .await
    }

    pub async fn conversations_with_pending(
        &self,
        channel_key: &ChannelKey,
    ) -> Result<Vec<ConversationId>> {
        self.repos
            .conversation_record
            .conversations_with_pending(channel_key)
            .await
    }
}

fn snapshot_values(snap: &ConversationSnapshot) -> Result<ChapterWrite> {
    Ok(ChapterWrite {
        messages: serde_json::to_value(&snap.context_messages)?,
        context_meta: serde_json::to_value(snap.context_meta)?,
    })
}

fn restore_snapshot(record: &ConversationRecord) -> Result<ConversationSnapshot> {
    let cid = record.id;
    let messages = serde_json::from_value(record.messages.clone()).map_err(|e| {
        AppError::message(format!(
            "conversation {cid}: snapshot messages unreadable: {e}"
        ))
    })?;
    let context_meta = serde_json::from_value(record.context_meta.clone()).map_err(|e| {
        AppError::message(format!(
            "conversation {cid}: snapshot context_meta unreadable: {e}"
        ))
    })?;

    Ok(ConversationSnapshot {
        context_messages: messages,
        context_meta,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn record(messages: Value, context_meta: Value) -> ConversationRecord {
        ConversationRecord {
            id: ConversationId(42),
            messages,
            context_meta,
            updated_at: jiff::Timestamp::now(),
        }
    }

    #[test]
    fn corrupt_snapshot_is_an_error() {
        assert!(
            restore_snapshot(&record(
                serde_json::json!({"not": "a message list"}),
                serde_json::json!("not an object"),
            ))
            .is_err(),
            "an unreadable snapshot must not be replaced by defaults"
        );
    }

    #[test]
    fn a_snapshot_survives_its_own_write() {
        let snap = ConversationSnapshot {
            context_messages: Vec::new(),
            context_meta: ContextMeta {
                tokens: 3,
                turn_count: 1,
                step_count: 2,
                session: crate::domain::vo::SessionId::new(),
            },
        };
        let chapter = snapshot_values(&snap).expect("a snapshot writes");
        let back = restore_snapshot(&record(chapter.messages, chapter.context_meta))
            .expect("a written snapshot reads back");
        assert_eq!(back.context_meta, snap.context_meta);
    }
}
