use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};
use uuid::Uuid;

use super::{Account, Chat};
use crate::domain::vo::MemoryId;

#[derive(Debug, Clone, toasty::Model)]
#[table = "memory"]
pub struct Memory {
    #[key]
    pub id: uuid::Uuid,

    pub account_id: Option<i64>,
    #[belongs_to(key = account_id, references = id)]
    pub account: toasty::Deferred<Option<Account>>,

    pub chat_id: Option<i64>,
    #[belongs_to(key = chat_id, references = id)]
    pub chat: toasty::Deferred<Option<Chat>>,

    pub kind: String,
    pub content: String,
    pub importance: i32,
    pub meta: Option<toasty::Json<serde_json::Value>>,

    #[auto]
    pub created_at: jiff::Timestamp,
    #[auto]
    pub updated_at: jiff::Timestamp,
}

impl Memory {
    pub fn id_(&self) -> MemoryId {
        MemoryId(self.id)
    }

    pub fn new(kind: MemoryKind, content: String) -> Self {
        Self {
            id: Uuid::now_v7(),
            account_id: None,
            account: Default::default(),
            chat_id: None,
            chat: Default::default(),
            kind: kind.to_string(),
            content,
            importance: 1,
            meta: None,
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        }
    }

    pub fn memory_kind(&self) -> Option<MemoryKind> {
        self.kind.parse().ok()
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, IntoStaticStr, Serialize, Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    UserFact,
    Note,
    Knowledge,
}

impl MemoryKind {
    pub fn as_str(&self) -> &'static str {
        (*self).into()
    }
}
