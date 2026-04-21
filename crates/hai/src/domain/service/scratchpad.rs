use anyhow::Result;
use sqlx::PgPool;

use crate::domain::{entity::Scratchpad, repo::ScratchpadRepo};
use crate::util::token::count_tokens;

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
        let token_count = count_tokens(content) as i32;
        ScratchpadRepo::upsert(&self.pool, chat_id, content, token_count).await
    }
}
