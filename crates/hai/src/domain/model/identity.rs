#[derive(Debug, Clone, toasty::Model)]
#[table = "identity"]
pub struct Identity {
    #[key]
    pub id: uuid::Uuid,

    pub name: Option<String>,
    pub meta: Option<toasty::Json<serde_json::Value>>,

    #[auto]
    pub created_at: jiff::Timestamp,
    #[auto]
    pub updated_at: jiff::Timestamp,
}
