use crate::{
    agentcore::token::count_tokens,
    domain::{model::Scratchpad, vo::ChatId},
    error::Result,
};

#[derive(Debug)]
pub struct ScratchpadService {
    db: toasty::Db,
}

impl ScratchpadService {
    pub fn new(db: toasty::Db) -> Self {
        Self { db }
    }

    pub async fn get(&self, chat_id: ChatId) -> Result<Option<Scratchpad>> {
        Scratchpad::get_by_chat_id(&mut self.db.clone(), &chat_id.0)
            .await
            .map(Some)
            .or_else(|_| Ok(None))
    }

    pub async fn save(&self, chat_id: ChatId, content: &str) -> Result<Scratchpad> {
        let token_count = count_tokens(content) as i32;
        let now = jiff::Timestamp::now();
        let mut db = self.db.clone();

        if let Ok(mut existing) = Scratchpad::get_by_chat_id(&mut db, &chat_id.0).await {
            toasty::update!(existing {
                content,
                token_count
            })
            .exec(&mut self.db.clone())
            .await?;
            Ok(existing)
        } else {
            toasty::create!(Scratchpad {
                chat_id: chat_id.0,
                content,
                token_count,
                updated_at: now,
            })
            .exec(&mut self.db.clone())
            .await
            .map_err(Into::into)
        }
    }
}
