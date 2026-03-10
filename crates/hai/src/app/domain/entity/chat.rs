use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use strum::{Display, EnumString, IntoStaticStr};

/// 场所 (Chat)
#[derive(Debug, Clone, FromRow)]
pub struct Chat {
    pub id: i64,
    pub platform: String,
    pub external_id: String,
    pub chat_type: String,
    pub name: Option<String>,
    pub config: Option<serde_json::Value>,
    pub meta: Option<serde_json::Value>,
    pub created_at: jiff_sqlx::Timestamp,
    pub updated_at: jiff_sqlx::Timestamp,
}

impl Chat {
    pub fn chat_type(&self) -> ChatType {
        self.chat_type.parse().expect("Invalid chat type")
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at.to_jiff()
    }

    pub fn updated_at(&self) -> Timestamp {
        self.updated_at.to_jiff()
    }
}

/// 场所类型
#[derive(
    Debug, Copy, Clone, PartialEq, Eq, Display, EnumString, IntoStaticStr, Serialize, Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum ChatType {
    Private,
    Group,
    Supergroup,
    Channel,
}
