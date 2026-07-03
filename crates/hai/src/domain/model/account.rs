use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

use super::Identity;
use crate::domain::vo::AccountId;

#[derive(Debug, Clone, toasty::Model)]
#[table = "account"]
pub struct Account {
    #[key]
    #[auto(increment)]
    pub id: i64,

    #[index]
    pub identity_id: Option<uuid::Uuid>,
    #[belongs_to(key = identity_id, references = id)]
    pub identity: toasty::Deferred<Option<Identity>>,

    pub platform: String,
    pub external_id: String,
    pub meta: Option<toasty::Json<serde_json::Value>>,

    pub last_active_at: jiff::Timestamp,
    #[auto]
    pub created_at: jiff::Timestamp,
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
