use std::fmt;

use teloxide::{
    Bot, net::Download, prelude::Requester,
    types::{FileId, ReplyParameters},
};

use crate::error::{AppResultExt, ErrorKind, Result};

#[derive(Clone)]
pub struct TelegramService {
    bot: Bot,
    http: reqwest::Client,
}

impl TelegramService {
    pub fn new(bot: Bot) -> Self {
        Self {
            bot,
            http: reqwest::Client::new(),
        }
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

    pub async fn send_rich_message(
        &self,
        chat_id: i64,
        markdown: &str,
        reply_params: Option<&ReplyParameters>,
    ) -> Result<teloxide::types::Message> {
        let mut payload = serde_json::json!({
            "chat_id": chat_id,
            "rich_message": { "markdown": markdown },
        });
        if let Some(params) = reply_params {
            payload["reply_parameters"] = serde_json::to_value(params)?;
        }

        let url = format!("https://api.telegram.org/bot{}/sendRichMessage", self.bot.token());
        let resp: serde_json::Value = self
            .http
            .post(url)
            .json(&payload)
            .send()
            .await?
            .json()
            .await?;

        if resp["ok"].as_bool() != Some(true) {
            let desc = resp["description"].as_str().unwrap_or("unknown error");
            return Err(ErrorKind::External.msg(format!(
                "sendRichMessage failed: {desc}"
            )));
        }

        let msg: teloxide::types::Message =
            serde_json::from_value(resp["result"].clone()).map_err(|e| {
                ErrorKind::DataParse.msg(format!("sendRichMessage response parse: {e}"))
            })?;
        Ok(msg)
    }
}

impl fmt::Debug for TelegramService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelegramService").finish_non_exhaustive()
    }
}
