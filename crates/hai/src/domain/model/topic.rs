use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

use crate::error::{ErrorKind, Result};

#[derive(Debug, Clone, toasty::Model)]
#[table = "topic"]
pub struct Topic {
    #[key]
    pub id: uuid::Uuid,

    pub chat_id: i64,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub status: String,
    pub parent_topic_id: Option<uuid::Uuid>,
    pub token_count: i32,
    pub message_count: i32,
    pub meta: Option<toasty::Json<serde_json::Value>>,

    pub started_at: jiff::Timestamp,
    pub last_active_at: jiff::Timestamp,
    pub closed_at: Option<jiff::Timestamp>,

    #[auto]
    pub created_at: jiff::Timestamp,
    #[auto]
    pub updated_at: jiff::Timestamp,
}

impl Topic {
    pub fn status(&self) -> TopicStatus {
        self.status.parse().expect("Invalid status")
    }

    pub fn ensure_not_closed(&self) -> Result<()> {
        if self.status() == TopicStatus::Closed {
            return Err(ErrorKind::BadRequest.msg("Cannot modify a closed topic"));
        }
        Ok(())
    }
}

#[derive(
    Debug, Copy, Clone, PartialEq, Eq, Display, EnumString, IntoStaticStr, Serialize, Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum TopicStatus {
    Active,
    Closed,
}
