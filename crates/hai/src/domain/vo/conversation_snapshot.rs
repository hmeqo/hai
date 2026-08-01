use std::collections::HashSet;

use genai::chat::ChatMessage;
use uuid::Uuid;

use crate::domain::vo::{MessageId, Turn};

/// `Conversation` 的纯数据快照。`context_tokens` 不持久化，restore 时从 turns 重算。
#[derive(Clone)]
pub struct ConversationSnapshot {
    pub messages: Vec<ChatMessage>,
    pub turns: Vec<Turn>,
    pub since_id: MessageId,
    pub shown_memory_ids: HashSet<Uuid>,
    pub shown_topic_ids: HashSet<Uuid>,
}
