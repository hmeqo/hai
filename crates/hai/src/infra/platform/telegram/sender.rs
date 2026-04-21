use anyhow::Result;
use std::sync::Arc;
use teloxide::{
    Bot,
    prelude::*,
    types::{ChatAction, MessageId, ReplyParameters},
};

use crate::agent::event::{BotSignal, GroupTrigger};
use crate::{
    domain::{
        service::{MessageService, PlatformService},
        vo::TelegramContentPart,
    },
    infra::platform::telegram::BotIdentity,
};

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
        bot_identity: BotIdentity,
    ) -> Result<()> {
        while let Some(signal) = signal_rx.recv().await {
            let sender = Arc::clone(&self);
            let model = model.clone();
            let account_id = bot_identity.account_id;
            tokio::spawn(async move {
                if let Err(e) = sender.handle_signal(signal, &model, account_id).await {
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
            BotSignal::Typing { chat_id } => {
                let platform_chat_id = self.resolve_platform_chat_id(chat_id).await?;
                let _ = self
                    .bot
                    .send_chat_action(ChatId(platform_chat_id), ChatAction::Typing)
                    .await;
            }
            BotSignal::SendMessage {
                chat_id,
                content,
                platform_reply_to_id,
                ..
            } => {
                let platform_chat_id = self.resolve_platform_chat_id(chat_id).await?;

                let mut send_req = self
                    .bot
                    .send_message(ChatId(platform_chat_id), content.clone());

                if let Some(msg_id) = platform_reply_to_id
                    && let Some(msg) = self.message_service.get_message_by_id(msg_id).await?
                    && let Some(id) = msg.external_id.map(|id| id.parse::<i32>()).transpose()?
                {
                    send_req = send_req.reply_parameters(ReplyParameters::new(MessageId(id)));
                }

                let sent_msg = send_req.await?;

                let content_value =
                    serde_json::to_value(vec![TelegramContentPart::Text { text: content }])?;
                self.message_service
                    .save_agent_message(
                        chat_id,
                        Some(bot_account_id),
                        content_value,
                        model,
                        0,
                        platform_reply_to_id,
                        Some(&sent_msg.id.0.to_string()),
                        Some(jiff::Timestamp::from_second(sent_msg.date.timestamp())?.into()),
                    )
                    .await?;

                self.group_trigger.on_agent_replied(chat_id);
            }
        }

        Ok(())
    }

    /// 通过内部 chat_id 查找 Telegram 平台的外部 chat id
    async fn resolve_platform_chat_id(&self, chat_id: i64) -> Result<i64> {
        self.platform_service
            .get_chat_by_id(chat_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Chat not found for internal id: {}", chat_id))
            .and_then(|chat| chat.external_id.parse::<i64>().map_err(Into::into))
    }
}
