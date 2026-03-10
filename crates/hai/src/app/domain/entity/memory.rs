use jiff::Timestamp;
use pgvector::Vector;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use strum::{Display, EnumString, IntoStaticStr};
use uuid::Uuid;

/// 统一记忆库 (Memory)
#[derive(Debug, Clone, FromRow)]
pub struct Memory {
    pub id: Uuid,
    pub account_id: Option<i64>,
    pub chat_id: Option<i64>,
    pub topic_id: Option<Uuid>,
    /// 记忆类型
    pub type_: String,
    pub content: String,
    pub embedding: Option<Vector>,
    pub source_message_id: Option<i64>,
    pub importance: i32,
    pub meta: Option<serde_json::Value>,
    pub last_accessed_at: jiff_sqlx::Timestamp,
    pub created_at: jiff_sqlx::Timestamp,
    pub updated_at: jiff_sqlx::Timestamp,
}

impl Memory {
    pub fn new(type_: MemoryType, content: String) -> Self {
        let now = jiff_sqlx::Timestamp::from(jiff::Timestamp::now());
        Self {
            id: Uuid::new_v4(),
            account_id: None,
            chat_id: None,
            topic_id: None,
            type_: type_.to_string(),
            content,
            embedding: None,
            source_message_id: None,
            importance: 1,
            meta: None,
            last_accessed_at: now,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn memory_type(&self) -> MemoryType {
        self.type_.parse().expect("Invalid memory type")
    }

    pub fn last_accessed_at(&self) -> Timestamp {
        self.last_accessed_at.to_jiff()
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at.to_jiff()
    }

    pub fn updated_at(&self) -> Timestamp {
        self.updated_at.to_jiff()
    }
}

/// 记忆类型
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, IntoStaticStr, Serialize, Deserialize,
)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    UserFact,
    AgentNote,
    Knowledge,
    Rule,
}

impl MemoryType {
    pub fn needs_embedding(&self) -> bool {
        matches!(self, MemoryType::UserFact | MemoryType::Knowledge)
    }
}
