use teloxide::{Bot, types::FileId};

use super::util;
use crate::error::Result;

#[derive(Debug, Clone)]
pub struct TelegramService {
    bot: Bot,
}

impl TelegramService {
    pub fn new(bot: Bot) -> Self {
        Self { bot }
    }

    pub async fn download(&self, file_id: &str) -> Result<Vec<u8>> {
        util::download_file(&self.bot, file_id).await
    }

    pub async fn get_file_url(&self, file_id: &str) -> Result<String> {
        util::get_file_url(&self.bot, FileId(file_id.to_string())).await
    }
}
