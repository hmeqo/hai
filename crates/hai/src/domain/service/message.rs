use uuid::Uuid;

use crate::{
    domain::{
        model::{Message, MessageStatus},
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
}

pub(crate) struct UpsertMessageParams {
    pub chat_id: i64,
    pub external_id: Option<String>,
    pub role: String,
    pub content: toasty::Json<serde_json::Value>,
    pub account_id: Option<i64>,
    pub interaction_status: String,
    pub reply_to_id: Option<i64>,
    pub meta: toasty::Json<serde_json::Value>,
    pub sent_at: Option<jiff::Timestamp>,
    pub topic_id: Option<Uuid>,
}

#[derive(Debug)]
pub struct MessageService {
    db: toasty::Db,
}

impl MessageService {
    pub fn new(db: toasty::Db) -> Self {
        Self { db }
    }

    async fn upsert_by_external_id(&self, params: UpsertMessageParams) -> Result<Message> {
        let mut db = self.db.clone();
        if let Some(ref ext_id) = params.external_id
            && let Some(mut existing) = Message::filter(
                Message::fields()
                    .chat_id()
                    .eq(params.chat_id)
                    .and(Message::fields().external_id().eq(Some(ext_id.clone()))),
            )
            .first()
            .exec(&mut db)
            .await?
        {
            toasty::update!(existing {
                content: params.content,
                meta: params.meta,
                interaction_status: params.interaction_status,
            })
            .exec(&mut db)
            .await?;
            return Ok(existing);
        }
        toasty::create!(Message {
            chat_id: params.chat_id,
            account_id: params.account_id,
            role: params.role,
            content: params.content,
            topic_id: params.topic_id,
            interaction_status: params.interaction_status,
            reply_to_id: params.reply_to_id,
            external_id: params.external_id,
            meta: params.meta,
            sent_at: params.sent_at,
        })
        .exec(&mut db)
        .await
        .map_err(Into::into)
    }

    pub async fn save_user_message(&self, msg: NewUserMessage) -> Result<Message> {
        self.upsert_by_external_id(UpsertMessageParams {
            chat_id: msg.chat_id.0,
            external_id: Some(msg.external_id),
            role: "user".into(),
            content: toasty::Json(msg.content),
            account_id: Some(msg.account_id),
            interaction_status: MessageStatus::Unread.as_str().into(),
            reply_to_id: msg.reply_to_id,
            meta: toasty::Json(serde_json::to_value(&msg.meta).unwrap_or(serde_json::Value::Null)),
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
            content: toasty::Json(msg.content),
            account_id: msg.account_id,
            interaction_status: MessageStatus::Seen.as_str().into(),
            reply_to_id: msg.reply_to_id,
            meta: toasty::Json(serde_json::to_value(&meta).unwrap_or(serde_json::Value::Null)),
            sent_at: msg.sent_at,
            topic_id: None,
        })
        .await
    }

    pub async fn get_context_messages(
        &self,
        chat_id: ChatId,
        min_count: i64,
    ) -> Result<(Vec<Message>, i64)> {
        let mut db = self.db.clone();
        let cid = chat_id.0;

        let unread: Vec<Message> = Message::filter(
            Message::fields()
                .chat_id()
                .eq(cid)
                .and(Message::fields().interaction_status().eq("unread")),
        )
        .order_by(Message::fields().id().desc())
        .exec(&mut db)
        .await?;

        let mut unread = unread;
        if (unread.len() as i64) < min_count {
            let need = min_count as usize - unread.len();
            let known: std::collections::HashSet<i64> = unread.iter().map(|m| m.id).collect();
            let history: Vec<Message> = Message::filter(
                Message::fields()
                    .chat_id()
                    .eq(cid)
                    .and(Message::fields().interaction_status().ne("unread")),
            )
            .order_by(Message::fields().id().desc())
            .limit(need as usize)
            .exec(&mut db)
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
        limit: i64,
    ) -> Result<Vec<Message>> {
        let sid = since_id.map(|s| s.0).unwrap_or(-1);
        let mut msgs: Vec<Message> = Message::filter(
            Message::fields()
                .chat_id()
                .eq(chat_id.0)
                .and(Message::fields().id().gt(sid)),
        )
        .order_by(Message::fields().id().desc())
        .limit(limit as usize)
        .exec(&mut self.db.clone())
        .await?;
        msgs.reverse();
        Ok(msgs)
    }

    pub async fn get_unread_messages(&self, chat_id: ChatId, limit: i64) -> Result<Vec<Message>> {
        let mut msgs: Vec<Message> = Message::filter(
            Message::fields()
                .chat_id()
                .eq(chat_id.0)
                .and(Message::fields().interaction_status().eq("unread")),
        )
        .order_by(Message::fields().id().desc())
        .limit(limit as usize)
        .exec(&mut self.db.clone())
        .await?;
        msgs.reverse();
        Ok(msgs)
    }

    pub async fn get_read_messages(&self, chat_id: ChatId, limit: i64) -> Result<Vec<Message>> {
        let mut msgs: Vec<Message> = Message::filter(
            Message::fields()
                .chat_id()
                .eq(chat_id.0)
                .and(Message::fields().interaction_status().ne("unread")),
        )
        .order_by(Message::fields().id().desc())
        .limit(limit as usize)
        .exec(&mut self.db.clone())
        .await?;
        msgs.reverse();
        Ok(msgs)
    }

    pub async fn get_message_by_id(&self, id: MessageId) -> Result<Option<Message>> {
        Message::get_by_id(&mut self.db.clone(), &id.0)
            .await
            .map(Some)
            .or_else(|e| {
                tracing::warn!(msg_id = %id, "get_message_by_id failed: {e}");
                Ok(None)
            })
    }

    pub async fn get_messages_by_ids(&self, ids: &[MessageId]) -> Result<Vec<Message>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let raw: Vec<i64> = ids.iter().map(|id| id.0).collect();
        Message::filter(Message::fields().id().in_list(raw))
            .exec(&mut self.db.clone())
            .await
            .map_err(Into::into)
    }

    pub async fn get_message_id_by_external_id(
        &self,
        chat_id: ChatId,
        external_id: &str,
    ) -> Result<Option<MessageId>> {
        let msg = Message::filter(
            Message::fields().chat_id().eq(chat_id.0).and(
                Message::fields()
                    .external_id()
                    .eq(Some(external_id.to_string())),
            ),
        )
        .first()
        .exec(&mut self.db.clone())
        .await?;
        Ok(msg.map(|m| MessageId(m.id)))
    }

    pub async fn count_unread_by_chat(&self, chat_id: ChatId) -> Result<u64> {
        Message::filter(
            Message::fields()
                .chat_id()
                .eq(chat_id.0)
                .and(Message::fields().interaction_status().eq("unread")),
        )
        .count()
        .exec(&mut self.db.clone())
        .await
        .map_err(Into::into)
    }

    pub async fn mark_replied(&self, ids: &[MessageId]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let raw: Vec<i64> = ids.iter().map(|id| id.0).collect();
        Message::filter(Message::fields().id().in_list(raw))
            .update()
            .interaction_status(MessageStatus::Replied.as_str())
            .exec(&mut self.db.clone())
            .await?;
        Ok(())
    }

    pub async fn mark_unread_seen(&self, ids: &[MessageId]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let raw: Vec<i64> = ids.iter().map(|id| id.0).collect();
        Message::filter(
            Message::fields()
                .id()
                .in_list(raw)
                .and(Message::fields().interaction_status().eq("unread")),
        )
        .update()
        .interaction_status(MessageStatus::Seen.as_str())
        .exec(&mut self.db.clone())
        .await?;
        Ok(())
    }

    pub async fn update_meta(&self, id: MessageId, meta: Option<serde_json::Value>) -> Result<()> {
        let mut db = self.db.clone();
        Message::filter_by_id(id.0)
            .update()
            .meta(meta.map(toasty::Json))
            .exec(&mut db)
            .await?;
        Ok(())
    }

    pub async fn find_attachment(
        &self,
        attachment_id: Uuid,
    ) -> Result<Option<(Message, TelegramContentPart)>> {
        let messages: Vec<Message> = Message::all()
            .order_by(Message::fields().id().desc())
            .limit(200)
            .exec(&mut self.db.clone())
            .await?;

        for msg in messages {
            if let Ok(parts) =
                serde_json::from_value::<Vec<TelegramContentPart>>(msg.content.0.clone())
                && let Some(part) = parts
                    .into_iter()
                    .find(|p| p.attachment_id() == Some(attachment_id))
            {
                return Ok(Some((msg, part)));
            }
        }
        Ok(None)
    }
}
