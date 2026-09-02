use uuid::Uuid;

use crate::{
    domain::{
        model::{Message, MessageStatus},
        repo::Repos,
        vo::{AgentMessageMeta, ChatId, MessageId, MessageMeta, TelegramContentPart},
    },
    error::Result,
};

pub struct NewUserMessage {
    pub chat_id: ChatId,
    pub account_id: i64,
    pub content: serde_json::Value,
    pub external_id: String,
    pub reply_to_id: Option<i64>,
    pub meta: MessageMeta,
    pub sent_at: Option<jiff::Timestamp>,
}

pub struct NewAgentMessage {
    pub chat_id: ChatId,
    pub account_id: Option<i64>,
    pub content: serde_json::Value,
    pub model: String,
    pub reply_to_id: Option<i64>,
    pub external_id: Option<String>,
    pub sent_at: Option<jiff::Timestamp>,
    pub topic_id: Option<Uuid>,
}

pub(crate) struct UpsertMessageParams {
    pub chat_id: i64,
    pub external_id: Option<String>,
    pub role: String,
    pub content: serde_json::Value,
    pub account_id: Option<i64>,
    pub interaction_status: String,
    pub reply_to_id: Option<i64>,
    pub meta: serde_json::Value,
    pub sent_at: Option<jiff::Timestamp>,
    pub topic_id: Option<Uuid>,
}

#[derive(Debug)]
pub struct MessageService {
    repos: Repos,
}

impl MessageService {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }

    async fn upsert_by_external_id(&self, params: UpsertMessageParams) -> Result<Message> {
        if let Some(ref ext_id) = params.external_id
            && let Some(existing) = self
                .repos
                .message
                .find_by_chat_external_id(params.chat_id, ext_id)
                .await?
        {
            self.repos
                .message
                .update_content_meta_status(
                    existing.id,
                    params.content,
                    params.meta,
                    &params.interaction_status,
                )
                .await?;
            return Ok(existing);
        }
        self.repos
            .message
            .create(
                params.chat_id,
                params.account_id,
                &params.role,
                params.content,
                params.topic_id,
                &params.interaction_status,
                params.reply_to_id,
                params.external_id.as_deref(),
                params.meta,
                params.sent_at,
            )
            .await
    }

    pub async fn save_user_message(&self, msg: NewUserMessage) -> Result<Message> {
        self.upsert_by_external_id(UpsertMessageParams {
            chat_id: msg.chat_id.0,
            external_id: Some(msg.external_id),
            role: "user".into(),
            content: msg.content,
            account_id: Some(msg.account_id),
            interaction_status: MessageStatus::Unread.as_str().into(),
            reply_to_id: msg.reply_to_id,
            meta: serde_json::to_value(&msg.meta).unwrap_or(serde_json::Value::Null),
            sent_at: msg.sent_at,
            topic_id: None,
        })
        .await
    }

    pub async fn save_agent_message(&self, msg: NewAgentMessage) -> Result<Message> {
        let meta = AgentMessageMeta { model: msg.model };
        self.upsert_by_external_id(UpsertMessageParams {
            chat_id: msg.chat_id.0,
            external_id: msg.external_id,
            role: "assistant".into(),
            content: msg.content,
            account_id: msg.account_id,
            interaction_status: MessageStatus::Seen.as_str().into(),
            reply_to_id: msg.reply_to_id,
            meta: serde_json::to_value(&meta).unwrap_or(serde_json::Value::Null),
            sent_at: msg.sent_at,
            topic_id: msg.topic_id,
        })
        .await
    }

    pub async fn get_context_messages(
        &self,
        chat_id: ChatId,
        min_count: i64,
    ) -> Result<(Vec<Message>, i64)> {
        let cid = chat_id.0;

        let mut unread = self
            .repos
            .message
            .by_chat_status_desc(cid, "unread", None)
            .await?;
        if (unread.len() as i64) < min_count {
            let need = min_count as usize - unread.len();
            let known: std::collections::HashSet<i64> = unread.iter().map(|m| m.id).collect();
            let history: Vec<Message> = self
                .repos
                .message
                .by_chat_status_ne_desc(cid, "unread", Some(need as i64))
                .await?;
            for m in history.into_iter().rev() {
                if !known.contains(&m.id) {
                    unread.push(m);
                }
            }
        }

        unread.sort_by_key(|m| m.id);
        let last_id = unread.last().map(|m| m.id).unwrap_or(-1);
        Ok((unread, last_id))
    }

    pub async fn get_messages_window(
        &self,
        chat_id: ChatId,
        since_id: Option<MessageId>,
    ) -> Result<Vec<Message>> {
        let sid = since_id.map(|s| s.0).unwrap_or(-1);
        let mut msgs = self
            .repos
            .message
            .by_chat_after_desc(chat_id.0, sid)
            .await?;
        msgs.reverse();
        Ok(msgs)
    }

    pub async fn get_unread_messages(&self, chat_id: ChatId, limit: i64) -> Result<Vec<Message>> {
        let mut msgs = self
            .repos
            .message
            .by_chat_status_desc(chat_id.0, "unread", Some(limit))
            .await?;
        msgs.reverse();
        Ok(msgs)
    }

    pub async fn get_read_messages(&self, chat_id: ChatId, limit: i64) -> Result<Vec<Message>> {
        let mut msgs = self
            .repos
            .message
            .by_chat_status_ne_desc(chat_id.0, "unread", Some(limit))
            .await?;
        msgs.reverse();
        Ok(msgs)
    }

    pub async fn get_message_by_id(&self, id: MessageId) -> Result<Option<Message>> {
        self.repos.message.by_id(id.0).await.or_else(|e| {
            tracing::warn!(msg_id = %id, "get_message_by_id failed: {e}");
            Ok(None)
        })
    }

    pub async fn get_messages_by_ids(&self, ids: &[MessageId]) -> Result<Vec<Message>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let raw: Vec<i64> = ids.iter().map(|id| id.0).collect();
        self.repos.message.by_ids(&raw).await
    }

    pub async fn get_message_id_by_external_id(
        &self,
        chat_id: ChatId,
        external_id: &str,
    ) -> Result<Option<MessageId>> {
        let id = self
            .repos
            .message
            .id_by_chat_external_id(chat_id.0, external_id)
            .await?;
        Ok(id.map(MessageId))
    }

    pub async fn count_unread_by_chat(&self, chat_id: ChatId) -> Result<u64> {
        Ok(self.repos.message.count_unread_by_chat(chat_id.0).await? as u64)
    }

    pub async fn mark_unread_seen(&self, ids: &[MessageId]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let raw: Vec<i64> = ids.iter().map(|id| id.0).collect();
        self.repos.message.mark_unread_seen(&raw).await
    }

    pub async fn update_meta(&self, id: MessageId, meta: Option<serde_json::Value>) -> Result<()> {
        self.repos.message.update_meta(id.0, meta).await
    }

    /// 按附件 UUID 查找消息（JSONB `@>` 过滤下推到 SQL，命中后仅反序列化该条）。
    pub async fn find_attachment(
        &self,
        attachment_id: Uuid,
    ) -> Result<Option<(Message, TelegramContentPart)>> {
        let needle = serde_json::json!([{ "attachment_id": attachment_id }]);
        let Some(msg) = self.repos.message.find_by_attachment(&needle).await? else {
            return Ok(None);
        };
        if let Ok(parts) = serde_json::from_value::<Vec<TelegramContentPart>>(msg.content.clone())
            && let Some(part) = parts
                .into_iter()
                .find(|p| p.attachment_id() == Some(attachment_id))
        {
            return Ok(Some((msg, part)));
        }
        Ok(None)
    }
}
