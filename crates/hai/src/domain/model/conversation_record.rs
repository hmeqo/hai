use crate::domain::vo::ConversationId;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ConversationRecord {
    pub id: ConversationId,
    pub messages: serde_json::Value,
    pub context_meta: serde_json::Value,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub updated_at: jiff::Timestamp,
}
