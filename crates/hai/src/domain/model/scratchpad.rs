#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Scratchpad {
    pub chat_id: i64,

    pub content: String,
    pub token_count: i32,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub updated_at: jiff::Timestamp,
}

impl Scratchpad {
    pub fn is_empty(&self) -> bool {
        self.content.trim().is_empty()
    }
}
