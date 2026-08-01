use super::Chat;

#[derive(Debug, Clone, toasty::Model)]
#[table = "conversation"]
pub struct ConversationRecord {
    #[key]
    pub chat_id: i64,
    #[belongs_to]
    pub chat: toasty::Deferred<Chat>,

    pub messages: toasty::Json<serde_json::Value>,
    pub since_id: i64,
    pub shown_memory_ids: toasty::Json<serde_json::Value>,
    pub shown_topic_ids: toasty::Json<serde_json::Value>,
    pub turns: toasty::Json<serde_json::Value>,

    #[auto]
    pub updated_at: jiff::Timestamp,
}
