use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

use crate::domain::vo::ChatId;

#[derive(Debug, Clone, toasty::Model)]
#[table = "chat"]
pub struct Chat {
    #[key]
    #[auto(increment)]
    pub id: i64,

    pub platform: String,
    pub external_id: String,
    pub chat_type: String,
    pub name: Option<String>,
    pub config: Option<toasty::Json<serde_json::Value>>,
    pub meta: Option<toasty::Json<serde_json::Value>>,

    #[auto]
    pub created_at: jiff::Timestamp,
    #[auto]
    pub updated_at: jiff::Timestamp,
}

impl Chat {
    pub fn id_(&self) -> ChatId {
        ChatId(self.id)
    }

    pub fn chat_type(&self) -> Option<ChatType> {
        self.chat_type.parse().ok()
    }
}

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
