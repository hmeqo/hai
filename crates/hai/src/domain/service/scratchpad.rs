use crate::{
    domain::{model::Scratchpad, repo::Repos, vo::ChatId},
    error::Result,
};

#[derive(Debug)]
pub struct ScratchpadService {
    repos: Repos,
}

impl ScratchpadService {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }

    pub async fn get(&self, chat_id: ChatId) -> Result<Option<Scratchpad>> {
        self.repos
            .scratchpad
            .find_by_chat_id(chat_id.0)
            .await
            .or_else(|e| {
                tracing::warn!(%chat_id, "Failed to get scratchpad: {e}");
                Ok(None)
            })
    }

    pub async fn save(&self, chat_id: ChatId, content: &str) -> Result<Scratchpad> {
        let cid = chat_id.0;
        if self.repos.scratchpad.find_by_chat_id(cid).await?.is_some() {
            self.repos.scratchpad.update_content(cid, content).await
        } else {
            self.repos
                .scratchpad
                .create(cid, content, 0, jiff::Timestamp::now())
                .await
        }
    }
}
