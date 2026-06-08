use teloxide::{Bot, net::Download, prelude::Requester, types::FileId};

use crate::error::{AppResultExt, ErrorKind, Result};

#[derive(Debug, Clone)]
pub struct TelegramService {
    bot: Bot,
}

impl TelegramService {
    pub fn new(bot: Bot) -> Self {
        Self { bot }
    }

    pub async fn download(&self, file_id: &str) -> Result<Vec<u8>> {
        let file = self.bot.get_file(FileId(file_id.to_string())).await?;
        let mut data = Vec::new();
        self.bot
            .download_file(&file.path, &mut data)
            .await
            .err_kind_msg(ErrorKind::BadRequest, "Failed to download file")?;
        Ok(data)
    }

    pub async fn get_file_url(&self, file_id: &str) -> Result<String> {
        let file = self.bot.get_file(FileId(file_id.to_string())).await?;
        Ok(format!(
            "https://api.telegram.org/file/bot{}/{}",
            self.bot.token(),
            file.path
        ))
    }
}
