use jiff::Timestamp;
use strum::{Display, EnumString, IntoStaticStr};

use super::*;
use crate::domain::vo::MessageId;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Message {
    pub id: i64,
    pub chat_id: i64,

    pub account_id: Option<i64>,

    pub role: String,
    pub content: serde_json::Value,

    pub topic_id: Option<uuid::Uuid>,

    pub interaction_status: String,
    pub reply_to_id: Option<i64>,

    pub external_id: Option<String>,
    pub meta: Option<serde_json::Value>,
    #[sqlx(try_from = "SqlxNullableTimestamp")]
    pub sent_at: Option<jiff::Timestamp>,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub created_at: jiff::Timestamp,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub updated_at: jiff::Timestamp,
}

impl Message {
    pub fn id_(&self) -> MessageId {
        MessageId(self.id)
    }

    pub fn active_at(&self) -> Timestamp {
        self.sent_at.unwrap_or(self.created_at)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Display, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum MessageStatus {
    Unread,
    Seen,
}

impl MessageStatus {
    pub fn as_str(&self) -> &'static str {
        self.into()
    }
}
