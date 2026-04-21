use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use strum::{Display, EnumString, IntoStaticStr};
use uuid::Uuid;

/// 消息流 (Message)
#[derive(Debug, Clone, FromRow)]
pub struct Message {
    pub id: i64,
    pub chat_id: i64,
    pub account_id: Option<i64>,
    pub role: String,
    pub content: serde_json::Value,
    pub topic_id: Option<Uuid>,
    pub interaction_status: String,
    pub reply_to_id: Option<i64>,
    pub external_id: Option<String>,
    pub meta: Option<serde_json::Value>,
    pub token_count: Option<i32>,
    pub sent_at: Option<jiff_sqlx::Timestamp>,
    pub created_at: jiff_sqlx::Timestamp,
    pub updated_at: jiff_sqlx::Timestamp,
}

impl Message {
    pub fn status(&self) -> MessageStatus {
        self.interaction_status.parse().expect("Invalid status")
    }

    pub fn sent_at(&self) -> Option<Timestamp> {
        self.sent_at.as_ref().map(|t| t.to_jiff())
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at.to_jiff()
    }

    pub fn active_at_sqlx(&self) -> jiff_sqlx::Timestamp {
        self.sent_at.unwrap_or(self.created_at)
    }

    pub fn active_at(&self) -> Timestamp {
        self.active_at_sqlx().to_jiff()
    }

    pub fn updated_at(&self) -> Timestamp {
        self.updated_at.to_jiff()
    }
}

/// 消息处理状态
#[derive(
    Debug, Clone, PartialEq, Eq, Display, EnumString, IntoStaticStr, Serialize, Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    Pending,
    Replied,
    Seen,
}
