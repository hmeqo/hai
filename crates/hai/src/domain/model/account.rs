use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

#[derive(Debug, Clone, toasty::Model)]
#[table = "account"]
pub struct Account {
    #[key]
    #[auto(increment)]
    pub id: i64,

    #[index]
    pub identity_id: Option<uuid::Uuid>,
    pub platform: String,
    pub external_id: String,
    pub meta: Option<toasty::Json<serde_json::Value>>,

    pub last_active_at: jiff::Timestamp,
    #[auto]
    pub created_at: jiff::Timestamp,
    pub updated_at: jiff::Timestamp,
}

impl Account {
    pub fn platform(&self) -> Platform {
        self.platform.parse().expect("Invalid platform")
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
