use sqlx::PgPool;

use crate::{
    agentcore::token::count_tokens,
    domain::{entity::Scratchpad, repo::ScratchpadRepo},
    error::Result,
};

#[derive(Debug)]
pub struct ScratchpadService {
    pool: PgPool,
}

impl ScratchpadService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn get(&self, chat_id: i64) -> Result<Option<Scratchpad>> {
        ScratchpadRepo::find(&self.pool, chat_id).await
    }

    pub async fn save(&self, chat_id: i64, content: &str) -> Result<Scratchpad> {
        tracing::info!(chat_id, content, "Save scratchpad");
        let token_count = count_tokens(content) as i32;
        ScratchpadRepo::upsert(&self.pool, chat_id, content, token_count).await
    }
}
