use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use strum::{Display, EnumString, IntoStaticStr};
use uuid::Uuid;

/// 平台账号 (Account)
#[derive(Debug, Clone, FromRow)]
pub struct Account {
    pub id: i64,
    pub identity_id: Option<Uuid>,
    pub platform: String,
    pub external_id: String,
    pub meta: Option<serde_json::Value>,
    pub last_active_at: jiff_sqlx::Timestamp,
    pub created_at: jiff_sqlx::Timestamp,
    pub updated_at: jiff_sqlx::Timestamp,
}

impl Account {
    pub fn platform(&self) -> Platform {
        self.platform.parse().expect("Invalid platform")
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
}

/// 平台类型
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, IntoStaticStr, Serialize, Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Telegram,
    System,
    Qq,
}
