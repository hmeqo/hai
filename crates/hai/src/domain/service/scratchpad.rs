use crate::{
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
            .or_else(|e| {
                tracing::warn!(%chat_id, "Failed to get scratchpad: {e}");
                Ok(None)
            })
    }

    pub async fn save(&self, chat_id: ChatId, content: &str) -> Result<Scratchpad> {
        let now = jiff::Timestamp::now();
        let mut db = self.db.clone();

        if let Ok(mut existing) = Scratchpad::get_by_chat_id(&mut db, &chat_id.0).await {
            toasty::update!(existing { content }).exec(&mut db).await?;
            Ok(existing)
        } else {
            toasty::create!(Scratchpad {
                chat_id: chat_id.0,
                content,
                token_count: 0,
                updated_at: now,
            })
            .exec(&mut db)
            .await
            .map_err(Into::into)
        }
    }
}
