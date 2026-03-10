use jiff::Timestamp;
use pgvector::Vector;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use strum::{Display, EnumString, IntoStaticStr};
use uuid::Uuid;

/// 话题流 (Topic)
#[derive(Debug, Clone, FromRow)]
pub struct Topic {
    pub id: Uuid,
    pub chat_id: i64,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub embedding: Option<Vector>,
    pub status: String,
    pub parent_topic_id: Option<Uuid>,
    pub token_count: i32,
    pub message_count: i32,
    pub meta: Option<serde_json::Value>,
    pub started_at: jiff_sqlx::Timestamp,
    pub created_at: jiff_sqlx::Timestamp,
    pub updated_at: jiff_sqlx::Timestamp,
    pub closed_at: Option<jiff_sqlx::Timestamp>,
    pub last_active_at: jiff_sqlx::Timestamp,
}

impl Topic {
    pub fn status(&self) -> TopicStatus {
        self.status.parse().expect("Invalid status")
    }

    pub fn started_at(&self) -> Timestamp {
        self.started_at.to_jiff()
    }

    pub fn last_active_at(&self) -> Timestamp {
        self.last_active_at.to_jiff()
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at.to_jiff()
    }

    pub fn updated_at(&self) -> Timestamp {
        self.updated_at.to_jiff()
    }

    pub fn closed_at(&self) -> Option<Timestamp> {
        self.closed_at.as_ref().map(|t| t.to_jiff())
    }
}

/// 话题总结状态
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, Display, EnumString, IntoStaticStr, Serialize, Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum TopicStatus {
    Active,
    Closed,
    Paused,
}
