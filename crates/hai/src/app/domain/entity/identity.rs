use jiff::Timestamp;
use sqlx::FromRow;
use uuid::Uuid;

/// 统一身份 (Identity)
#[derive(Debug, Clone, FromRow)]
pub struct Identity {
    pub id: Uuid,
    pub name: Option<String>,
    pub meta: Option<serde_json::Value>,
    pub created_at: jiff_sqlx::Timestamp,
    pub updated_at: jiff_sqlx::Timestamp,
}

impl Identity {
    pub fn created_at(&self) -> Timestamp {
        self.created_at.to_jiff()
    }

    pub fn updated_at(&self) -> Timestamp {
        self.updated_at.to_jiff()
    }
}
