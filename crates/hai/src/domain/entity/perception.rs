use jiff::Timestamp;
use pgvector::Vector;
use sqlx::FromRow;
use uuid::Uuid;

use crate::domain::vo::Source;

/// 感知（资源的分析结果）
#[derive(Debug, Clone, FromRow)]
pub struct Perception {
    pub id: Uuid,
    pub source: serde_json::Value,
    pub parser: String,
    pub prompt: Option<String>,
    pub content: String,
    pub embedding: Option<Vector>,
    pub created_at: jiff_sqlx::Timestamp,
}

impl Perception {
    pub fn created_at(&self) -> Timestamp {
        self.created_at.to_jiff()
    }

    pub fn source(&self) -> Option<Source> {
        serde_json::from_value(self.source.clone()).ok()
    }
}
