use jiff::Timestamp;
use serde_json::Value;

use crate::domain::vo::ConversationId;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ConversationPreference {
    pub conversation_id: ConversationId,
    pub prefs: Value,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub updated_at: Timestamp,
}
