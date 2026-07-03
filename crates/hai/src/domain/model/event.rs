#[derive(Debug, Clone, toasty::Model)]
#[table = "event"]
pub struct Event {
    #[key]
    #[auto(increment)]
    pub seq: i64,

    pub domain: String,
    pub kind: String,

    #[index]
    pub chat_id: Option<i64>,

    pub payload: toasty::Json<serde_json::Value>,

    #[index]
    #[auto]
    pub created_at: jiff::Timestamp,
}
