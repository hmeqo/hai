use std::collections::HashMap;

use crate::{
    domain::{
        model::{Attachment, AttachmentHandle, Message, MessageRole, NewAttachment, NewMessage},
        repo::Repos,
        vo::{
            AccountId, AttachmentId, ChatId, ConversationId, LlmMessageMeta, MessageId,
            MessageMeta, PlatformMessageMeta, TimeRange, TopicId,
        },
    },
    error::Result,
};

/// One message the agent sends. Its platform extras travel with it so the row records what actually
/// left — an outgoing reply that pointed at an excerpt included.
pub struct NewAgentMessage {
    pub chat_id: ChatId,
    pub conversation_id: ConversationId,
    pub account_id: Option<AccountId>,
    pub text: String,
    pub attachments: Vec<NewAttachment>,
    pub model: String,
    pub platform_meta: Option<PlatformMessageMeta>,
    pub reply_to_id: Option<MessageId>,
    pub external_id: Option<String>,
    pub sent_at: Option<jiff::Timestamp>,
    pub topic_id: Option<TopicId>,
}

#[derive(Debug)]
pub struct MessageService {
    repos: Repos,
}

impl MessageService {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }

    /// Writes one message, refreshing the row a redelivery of the same external id names; the
    /// caller hands the shape the repo inserts, so nothing here re-lays it out.
    async fn upsert_by_external_id(&self, new: NewMessage) -> Result<Message> {
        let existing = match &new.external_id {
            Some(external_id) => {
                self.repos
                    .message
                    .find_by_chat_external_id(new.chat_id, external_id)
                    .await?
            }
            None => None,
        };
        match existing {
            Some(existing) => {
                self.repos
                    .message
                    .refresh(
                        &existing,
                        new.text,
                        new.meta,
                        existing.edited_at,
                        &new.attachments,
                    )
                    .await
            }
            None => self.repos.message.create(new).await,
        }
    }

    pub async fn save_agent_message(&self, msg: NewAgentMessage) -> Result<Message> {
        let meta = MessageMeta {
            platform: msg.platform_meta,
            llm: Some(LlmMessageMeta {
                model: msg.model,
                reasoning: None,
            }),
        };
        self.upsert_by_external_id(NewMessage {
            chat_id: msg.chat_id,
            conversation_id: msg.conversation_id,
            account_id: msg.account_id,
            role: MessageRole::Assistant,
            text: msg.text,
            attachments: msg.attachments,
            topic_id: msg.topic_id,
            reply_to_id: msg.reply_to_id,
            external_id: msg.external_id,
            meta: serde_json::to_value(&meta)?,
            sent_at: msg.sent_at,
            edited_at: None,
        })
        .await
    }

    /// Everything a quote may be copied from: the message's own text plus each attachment's caption,
    /// because that is what the context renders as the message's content.
    pub async fn quoted_content(&self, message: &Message) -> Result<String> {
        let attachments = self.repos.attachment.by_message_ids(&[message.id]).await?;
        Ok(message.content(&attachments))
    }

    pub async fn get_message_by_id(&self, id: MessageId) -> Result<Option<Message>> {
        self.repos.message.by_id(id).await
    }

    pub async fn get_messages_by_ids(&self, ids: &[MessageId]) -> Result<Vec<Message>> {
        self.repos.message.by_ids(ids).await
    }

    pub async fn recent_for_conversation(
        &self,
        conversation_id: ConversationId,
        limit: i64,
    ) -> Result<Vec<Message>> {
        self.repos
            .message
            .recent_for_conversation(conversation_id, limit)
            .await
    }

    pub async fn has_newer_from_user(&self, chat_id: ChatId, id: MessageId) -> Result<bool> {
        self.repos.message.has_newer_from_user(chat_id, id).await
    }

    pub async fn attachments_by_messages(
        &self,
        ids: &[MessageId],
    ) -> Result<HashMap<MessageId, Vec<Attachment>>> {
        let rows = self.repos.attachment.by_message_ids(ids).await?;
        let mut grouped: HashMap<MessageId, Vec<Attachment>> = HashMap::new();
        for row in rows {
            grouped.entry(row.message_id).or_default().push(row);
        }
        Ok(grouped)
    }

    pub async fn attachment_by_id(&self, id: AttachmentId) -> Result<Option<Attachment>> {
        self.repos.attachment.by_id(id).await
    }

    /// The messages of a window, ascending.
    pub async fn in_range(&self, chat_id: ChatId, range: TimeRange) -> Result<Vec<Message>> {
        self.repos.message.in_range(chat_id, range).await
    }

    /// The platform ids of a window's messages, for deleting them there.
    pub async fn external_ids_in_range(
        &self,
        conversation_id: ConversationId,
        chat_id: ChatId,
        range: TimeRange,
    ) -> Result<Vec<String>> {
        self.repos
            .message
            .external_ids_in_range(conversation_id, chat_id, range)
            .await
    }

    /// The platform files a window's messages carry, for the bytes cache and the transcripts.
    pub async fn attachment_handles_in_range(
        &self,
        conversation_id: ConversationId,
        chat_id: ChatId,
        range: TimeRange,
    ) -> Result<Vec<AttachmentHandle>> {
        self.repos
            .attachment
            .handles_in_range(conversation_id, chat_id, range)
            .await
    }
}
