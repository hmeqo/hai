use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

use crate::domain::vo::AccountId;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Account {
    pub id: i64,
    pub identity_id: Option<uuid::Uuid>,

    pub platform: String,
    pub external_id: String,
    pub meta: Option<serde_json::Value>,

    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub last_active_at: jiff::Timestamp,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub created_at: jiff::Timestamp,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub updated_at: jiff::Timestamp,
}

impl Account {
    pub fn id_(&self) -> AccountId {
        AccountId(self.id)
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, IntoStaticStr, Serialize, Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    System,
    Telegram,
    Qq,
}
