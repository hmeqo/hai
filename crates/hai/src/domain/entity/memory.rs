use jiff::Timestamp;
use pgvector::Vector;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use strum::{Display, EnumString, IntoStaticStr};
use uuid::Uuid;

/// 记忆关联引用
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryReferences {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topics: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

/// 统一记忆库 (Memory)
#[derive(Debug, Clone, FromRow)]
pub struct Memory {
    pub id: Uuid,
    pub account_id: Option<i64>,
    pub chat_id: Option<i64>,
    /// 记忆类型
    pub type_: String,
    pub content: String,
    pub embedding: Option<Vector>,
    pub importance: i32,
    pub subject: Option<String>,
    pub references: Option<serde_json::Value>,
    pub meta: Option<serde_json::Value>,
    pub last_accessed_at: jiff_sqlx::Timestamp,
    pub created_at: jiff_sqlx::Timestamp,
    pub updated_at: jiff_sqlx::Timestamp,
}

impl Memory {
    pub fn new(type_: MemoryType, content: String) -> Self {
        let now = jiff_sqlx::Timestamp::from(jiff::Timestamp::now());
        Self {
            id: Uuid::now_v7(),
            account_id: None,
            chat_id: None,
            type_: type_.to_string(),
            content,
            embedding: None,
            importance: 1,
            subject: None,
            references: None,
            meta: None,
            last_accessed_at: now,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn memory_type(&self) -> MemoryType {
        self.type_.parse().expect("Invalid memory type")
    }

    pub fn created_at(&self) -> Timestamp {
        self.created_at.to_jiff()
    }

    pub fn references(&self) -> Option<MemoryReferences> {
        self.references
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    pub fn set_references(&mut self, refs: MemoryReferences) {
        self.references = serde_json::to_value(&refs).ok();
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
