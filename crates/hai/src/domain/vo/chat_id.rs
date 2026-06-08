use std::fmt;

use serde::{Deserialize, Serialize};
use sqlx::Type;

/// 内部聊天标识符。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Type, Serialize, Deserialize)]
#[serde(transparent)]
#[sqlx(transparent)]
pub struct ChatId(pub i64);

impl ChatId {
    pub fn as_i64(self) -> i64 {
        self.0
    }
}

impl fmt::Display for ChatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<i64> for ChatId {
    fn from(id: i64) -> Self {
        Self(id)
    }
}

impl From<ChatId> for i64 {
    fn from(id: ChatId) -> Self {
        id.0
    }
}
