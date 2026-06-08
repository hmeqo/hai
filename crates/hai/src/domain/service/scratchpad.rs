use sqlx::PgPool;

use crate::{
    agentcore::token::count_tokens,
    domain::{entity::Scratchpad, repo::ScratchpadRepo, vo::ChatId},
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

    pub async fn get(&self, chat_id: ChatId) -> Result<Option<Scratchpad>> {
        ScratchpadRepo::find(&self.pool, chat_id).await
    }

    pub async fn save(&self, chat_id: ChatId, content: &str) -> Result<Scratchpad> {
        let token_count = count_tokens(content) as i32;
        ScratchpadRepo::upsert(&self.pool, chat_id, content, token_count).await
    }
}
