use crate::domain::vo::Source;

#[derive(Debug, Clone, toasty::Model)]
#[table = "perception"]
pub struct Perception {
    #[key]
    pub id: uuid::Uuid,

    pub source: toasty::Json<serde_json::Value>,
    pub parser: String,
    pub prompt: Option<String>,
    pub content: String,
    pub embedding: Option<toasty::Json<Vec<f32>>>,

    #[auto]
    pub created_at: jiff::Timestamp,
}

impl Perception {
    pub fn source(&self) -> Option<Source> {
        serde_json::from_value(self.source.0.clone()).ok()
    }
}
