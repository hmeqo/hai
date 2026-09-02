use crate::domain::vo::{PerceptionId, Source};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Perception {
    pub id: uuid::Uuid,

    pub source: serde_json::Value,
    pub parser: String,
    /// 针对性分析指令（可选）；None = 基础转写行，Some = 针对性判断行
    #[sqlx(rename = "prompt")]
    pub focus: Option<String>,
    pub content: String,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub created_at: jiff::Timestamp,
}

impl Perception {
    pub fn id_(&self) -> PerceptionId {
        PerceptionId(self.id)
    }

    pub fn source(&self) -> Option<Source> {
        serde_json::from_value(self.source.clone()).ok()
    }
}
