use anyhow::Result;
use std::sync::Arc;
use teloxide::{
    Bot,
    prelude::*,
    types::{MessageId, ReplyParameters},
};

use crate::app::domain::model::TelegramContentPart;
use crate::app::service::{MessageService, PlatformService};
use crate::trigger::{BotSignal, GroupTrigger};

pub struct TelegramSender {
    bot: Bot,
    platform_service: Arc<PlatformService>,
    message_service: Arc<MessageService>,
    group_trigger: Arc<GroupTrigger>,
}

impl TelegramSender {
    pub fn new(
        bot: Bot,
        platform_service: Arc<PlatformService>,
        message_service: Arc<MessageService>,
        group_trigger: Arc<GroupTrigger>,
    ) -> Self {
        Self {
            bot,
            platform_service,
            message_service,
            group_trigger,
        }
    }

    pub async fn run(
        self: Arc<Self>,
        mut signal_rx: tokio::sync::mpsc::UnboundedReceiver<BotSignal>,
        model: String,
        bot_account_id: i64,
    ) -> Result<()> {
        while let Some(signal) = signal_rx.recv().await {
            let sender = Arc::clone(&self);
            let model = model.clone();
            tokio::spawn(async move {
                if let Err(e) = sender.handle_signal(signal, &model, bot_account_id).await {
                    tracing::error!("TelegramSender handle_signal error: {e}");
                }
            });
        }
        Ok(())
    }

    async fn handle_signal(
        &self,
        signal: BotSignal,
        model: &str,
        bot_account_id: i64,
    ) -> Result<()> {
        match signal {
            BotSignal::SendMessage {
                chat_id,
                content,
                reply_to_platform_id,
                ..
            } => {
                let chat = self
                    .platform_service
                    .get_chat_by_id(chat_id)
                    .await?
                    .ok_or_else(|| {
                        anyhow::anyhow!("Chat not found for internal id: {}", chat_id)
                    })?;

                let platform_chat_id = chat.external_id.parse::<i64>()?;

                let mut reply_to_id = None;
                let mut send = self
                    .bot
                    .send_message(ChatId(platform_chat_id), content.clone());

                if let Some(reply_id_str) = &reply_to_platform_id {
                    reply_to_id = self
                        .message_service
                        .find_id_by_external_id(chat_id, reply_id_str)
                        .await?;
                    if let Ok(id) = reply_id_str.parse::<i32>() {
                        send = send.reply_parameters(ReplyParameters::new(MessageId(id)));
                    }
                }

                let sent_msg = send.await?;

                let content_value =
                    serde_json::to_value(vec![TelegramContentPart::Text { text: content }])?;
                self.message_service
                    .save_agent_message(
                        chat_id,
                        Some(bot_account_id),
                        content_value,
                        model,
                        0,
                        reply_to_id,
                        Some(&sent_msg.id.0.to_string()),
                        Some(jiff::Timestamp::from_second(sent_msg.date.timestamp())?.into()),
                    )
                    .await?;

                self.group_trigger.on_agent_sent(chat_id);
            }
        }

        Ok(())
    }
}
