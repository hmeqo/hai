use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

use super::*;
use crate::{
    domain::vo::TopicId,
    error::{ErrorKind, Result},
};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Topic {
    pub id: uuid::Uuid,
    pub chat_id: i64,

    pub title: Option<String>,
    pub summary: Option<String>,
    pub status: String,

    pub parent_topic_id: Option<uuid::Uuid>,

    pub meta: Option<serde_json::Value>,

    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub started_at: jiff::Timestamp,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub last_active_at: jiff::Timestamp,
    #[sqlx(try_from = "SqlxNullableTimestamp")]
    pub closed_at: Option<jiff::Timestamp>,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub created_at: jiff::Timestamp,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub updated_at: jiff::Timestamp,
}

impl Topic {
    pub fn id_(&self) -> TopicId {
        TopicId(self.id)
    }

    pub fn status(&self) -> Option<TopicStatus> {
        self.status.parse().ok()
    }

    pub fn ensure_not_closed(&self) -> Result<()> {
        if self.status() == Some(TopicStatus::Closed) {
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
