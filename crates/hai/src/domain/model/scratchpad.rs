#[derive(Debug, Clone, toasty::Model)]
#[table = "scratchpad"]
pub struct Scratchpad {
    #[key]
    pub chat_id: i64,

    pub content: String,
    pub token_count: i32,

    #[auto]
    pub updated_at: jiff::Timestamp,
}

impl Scratchpad {
    pub fn is_empty(&self) -> bool {
        self.content.trim().is_empty()
    }
}
