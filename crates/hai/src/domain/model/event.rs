#[derive(Debug, Clone, toasty::Model)]
#[table = "event"]
pub struct Event {
    #[key]
    #[auto(increment)]
    pub seq: i64,

    pub domain: String,

    pub payload: toasty::Json<serde_json::Value>,

    #[auto]
    pub created_at: jiff::Timestamp,
}
