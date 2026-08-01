use crate::{
    domain::{
        model::ConversationRecord,
        vo::{ChatId, ConversationSnapshot, MessageId},
    },
    error::Result,
};

#[derive(Debug)]
pub struct ConversationRecordService {
    db: toasty::Db,
}

impl ConversationRecordService {
    pub fn new(db: toasty::Db) -> Self {
        Self { db }
    }

    pub async fn get(&self, chat_id: ChatId) -> Result<Option<ConversationRecord>> {
        let mut db = self.db.clone();
        ConversationRecord::get_by_chat_id(&mut db, &chat_id.0)
            .await
            .map(Some)
            .or_else(|e| {
                tracing::warn!(%chat_id, "Failed to get conversation record: {e}");
                Ok(None)
            })
    }

    pub(crate) async fn save(&self, snap: &ConversationSnapshot, chat_id: ChatId) -> Result<()> {
        let now = jiff::Timestamp::now();
        let mut db = self.db.clone();
        let cid = chat_id.0;

        let messages = toasty::Json(serde_json::to_value(&snap.messages).unwrap_or_default());
        let turns = toasty::Json(serde_json::to_value(&snap.turns).unwrap_or_default());
        let shown_memory_ids = toasty::Json(serialize_uuids(&snap.shown_memory_ids));
        let shown_topic_ids = toasty::Json(serialize_uuids(&snap.shown_topic_ids));

        if let Ok(mut existing) = ConversationRecord::get_by_chat_id(&mut db, &cid).await {
            toasty::update!(existing {
                messages,
                turns,
                since_id: snap.since_id.0,
                shown_memory_ids,
                shown_topic_ids,
                updated_at: now,
            })
            .exec(&mut db)
            .await?;
        } else {
            toasty::create!(ConversationRecord {
                chat_id: cid,
                messages,
                turns,
                since_id: snap.since_id.0,
                shown_memory_ids,
                shown_topic_ids,
                updated_at: now,
            })
            .exec(&mut db)
            .await?;
        }
        Ok(())
    }

    pub(crate) fn restore(&self, record: &ConversationRecord) -> ConversationSnapshot {
        let messages: Vec<_> =
            serde_json::from_value(record.messages.0.clone()).unwrap_or_default();
        let turns: Vec<_> = serde_json::from_value(record.turns.0.clone()).unwrap_or_default();
        let shown_memory_ids = deserialize_uuids(&record.shown_memory_ids.0);
        let shown_topic_ids = deserialize_uuids(&record.shown_topic_ids.0);

        ConversationSnapshot {
            messages,
            turns,
            since_id: MessageId(record.since_id),
            shown_memory_ids,
            shown_topic_ids,
        }
    }
}

fn serialize_uuids(set: &std::collections::HashSet<uuid::Uuid>) -> serde_json::Value {
    serde_json::to_value(set.iter().collect::<Vec<_>>()).unwrap_or(serde_json::Value::Null)
}

fn deserialize_uuids(v: &serde_json::Value) -> std::collections::HashSet<uuid::Uuid> {
    let ids: Vec<uuid::Uuid> = serde_json::from_value(v.clone()).unwrap_or_default();
    ids.into_iter().collect()
}
