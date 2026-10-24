use crate::{
    domain::{
        repo::Repos,
        vo::{ChannelKey, ChatId, ConversationId},
    },
    error::{AppError, Result},
};

/// One operator-facing Conversation view.
#[derive(Debug, Clone)]
pub struct ConversationSummary {
    pub id: ConversationId,
    pub chats: Vec<ChatSummary>,
}

#[derive(Debug, Clone)]
pub struct ChatSummary {
    pub id: ChatId,
    pub alias: String,
    pub external_id: String,
    pub channel_key: ChannelKey,
}

#[derive(Debug)]
pub struct ConversationService {
    repos: Repos,
}

impl ConversationService {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }

    pub async fn attach(&self, chat_id: ChatId, conversation_id: ConversationId) -> Result<()> {
        self.repos
            .conversation_record
            .attach_chat(chat_id, conversation_id)
            .await
    }

    pub async fn detach(&self, chat_id: ChatId) -> Result<ConversationId> {
        self.repos.conversation_record.detach_chat(chat_id).await
    }

    pub async fn list(&self) -> Result<Vec<ConversationSummary>> {
        let rows = self.repos.conversation_record.list().await?;
        let mut summaries: Vec<ConversationSummary> = Vec::new();
        for row in rows {
            let summary = match summaries.last_mut() {
                Some(summary) if summary.id == row.conversation_id => summary,
                _ => {
                    summaries.push(ConversationSummary {
                        id: row.conversation_id,
                        chats: Vec::new(),
                    });
                    summaries.last_mut().expect("just pushed")
                }
            };
            match (row.chat_id, row.alias, row.external_id, row.channel_key) {
                (None, None, None, None) => {}
                (Some(id), Some(alias), Some(external_id), Some(channel_key)) => {
                    summary.chats.push(ChatSummary {
                        id,
                        alias,
                        external_id,
                        channel_key,
                    });
                }
                _ => {
                    return Err(AppError::message(format!(
                        "Conversation {} has an incomplete Chat row",
                        row.conversation_id
                    )));
                }
            }
        }
        Ok(summaries)
    }
}
