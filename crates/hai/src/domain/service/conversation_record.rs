use crate::{
    domain::{
        model::ConversationRecord,
        repo::Repos,
        vo::{ChatId, ContextMeta, ConversationSnapshot, MessageId},
    },
    error::Result,
};

#[derive(Debug)]
pub struct ConversationRecordService {
    repos: Repos,
}

impl ConversationRecordService {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }

    /// 契约式恢复：`Ok(None)` = 无记录（全新对话），`Err` = DB 违约（上抛，
    /// 不建会话。
    /// 用 filter 查询而非 `get_by_chat_id`：无行返回空 Vec（而非 not-found 错误），
    /// 天然区分"无记录"与"DB 错误"。
    pub async fn get(&self, chat_id: ChatId) -> Result<Option<ConversationRecord>> {
        self.repos
            .conversation_record
            .find_by_chat_id(chat_id.0)
            .await
    }

    pub(crate) async fn save(&self, snap: &ConversationSnapshot, chat_id: ChatId) -> Result<()> {
        let now = jiff::Timestamp::now();
        let cid = chat_id.0;

        let messages = serde_json::to_value(&snap.context_messages).unwrap_or_default();
        let state = serde_json::json!({
            "since_id": snap.since_id.0,
        });
        let context_meta = serde_json::json!({
            "shown_memory_ids": serialize_uuids(&snap.shown_memory_ids),
            "shown_topic_ids": serialize_uuids(&snap.shown_topic_ids),
            "tokens": snap.context_meta.tokens,
            "turn_count": snap.context_meta.turn_count,
            "step_count": snap.context_meta.step_count,
        });

        if self
            .repos
            .conversation_record
            .find_by_chat_id(cid)
            .await?
            .is_some()
        {
            self.repos
                .conversation_record
                .update(cid, messages, state, context_meta, now)
                .await?;
        } else {
            self.repos
                .conversation_record
                .create(cid, messages, state, context_meta, now)
                .await?;
        }
        Ok(())
    }

    pub(crate) fn restore(&self, record: &ConversationRecord) -> ConversationSnapshot {
        let messages: Vec<_> = serde_json::from_value(record.messages.clone()).unwrap_or_default();
        // state{since_id} / context_meta{shown ids, tokens, turn_count, step_count} 反序列化
        let state = record.state.clone();
        let meta = record.context_meta.clone();
        let since_id = MessageId(state.get("since_id").and_then(|v| v.as_i64()).unwrap_or(0));
        let shown_memory_ids = deserialize_uuids(
            &meta
                .get("shown_memory_ids")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        );
        let shown_topic_ids = deserialize_uuids(
            &meta
                .get("shown_topic_ids")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        );
        let context_meta: ContextMeta = serde_json::from_value(meta.clone()).unwrap_or_default();

        ConversationSnapshot {
            context_messages: messages,
            since_id,
            shown_memory_ids,
            shown_topic_ids,
            context_meta,
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
