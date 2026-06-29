use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};
use uuid::Uuid;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryReferences {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topics: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[derive(Debug, Clone, toasty::Model)]
#[table = "memory"]
pub struct Memory {
    #[key]
    pub id: uuid::Uuid,

    pub account_id: Option<i64>,
    pub chat_id: Option<i64>,
    #[column("type")]
    pub mem_type: String,
    pub content: String,
    pub embedding: Option<toasty::Json<Vec<f32>>>,
    pub importance: i32,
    pub subject: Option<String>,
    pub references: Option<toasty::Json<serde_json::Value>>,
    pub meta: Option<toasty::Json<serde_json::Value>>,

    pub last_accessed_at: jiff::Timestamp,
    #[auto]
    pub created_at: jiff::Timestamp,
    #[auto]
    pub updated_at: jiff::Timestamp,
}

impl Memory {
    pub fn new(type_: MemoryType, content: String) -> Self {
        Self {
            id: Uuid::now_v7(),
            account_id: None,
            chat_id: None,
            mem_type: type_.to_string(),
            content,
            embedding: None,
            importance: 1,
            subject: None,
            references: None,
            meta: None,
            last_accessed_at: jiff::Timestamp::now(),
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        }
    }

    pub fn memory_type(&self) -> MemoryType {
        self.mem_type.parse().expect("Invalid memory type")
    }

    pub fn references(&self) -> Option<MemoryReferences> {
        self.references
            .as_ref()
            .and_then(|r| serde_json::from_value(r.0.clone()).ok())
    }
}

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
