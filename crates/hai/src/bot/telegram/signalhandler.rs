use std::sync::Arc;

use teloxide::{
    prelude::*,
    types::{ChatAction, InputFile, MessageId, ReplyParameters},
};
use uuid::Uuid;

use crate::{
    agent::event::BotSignal,
    app::AppContext,
    domain::{
        service::NewAgentMessage,
        vo::{TelegramContentPart, VoiceMeta},
    },
    error::{ErrorKind, OptionAppExt, Result},
};

/// Telegram 消息发送器
///
/// 消费 `BotSignal`，调用 Telegram API 完成实际发送，并将结果持久化。
pub struct BotSignalHandler {
    ctx: AppContext,
}

impl BotSignalHandler {
    pub fn new(ctx: AppContext) -> Self {
        Self { ctx }
    }

    pub async fn run(
        self: Arc<Self>,
        mut signal_rx: tokio::sync::mpsc::UnboundedReceiver<BotSignal>,
    ) -> Result<()> {
        while let Some(signal) = signal_rx.recv().await {
            let sender = Arc::clone(&self);
            tokio::spawn(async move {
                if let Err(e) = sender.handle_signal(signal).await {
                    tracing::error!("TelegramSender handle_signal error: {e}");
                }
            });
        }
        Ok(())
    }

    async fn handle_signal(&self, signal: BotSignal) -> Result<()> {
        match signal {
            BotSignal::Typing { chat_id } => {
                let platform_chat_id = self.resolve_platform_chat_id(chat_id).await?;
                let _ = self
                    .ctx
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
                tracing::info!(
                    chat_id = chat_id,
                    content = %content,
                    "Bot sent message"
                );

                let platform_chat_id = self.resolve_platform_chat_id(chat_id).await?;
                let services = &self.ctx.db.srv;
                let bot = &self.ctx.bot;

                let mut send_req = bot.send_message(ChatId(platform_chat_id), content.clone());

                if let Some(msg_id) = platform_reply_to_id
                    && let Some(msg) = services.message.get_message_by_id(msg_id).await?
                    && let Some(id) = msg.external_id.map(|id| id.parse::<i32>()).transpose()?
                {
                    send_req = send_req.reply_parameters(ReplyParameters::new(MessageId(id)));
                }

                let sent_msg = send_req.await?;

                let model = self.ctx.agent.current_model();
                let bot_account_id = self.ctx.bot.account_id();
                let content_value =
                    serde_json::to_value(vec![TelegramContentPart::Text { text: content }])?;

                services
                    .message
                    .save_agent_message(NewAgentMessage {
                        chat_id,
                        account_id: Some(bot_account_id),
                        content: content_value,
                        model: &model,
                        tokens: 0,
                        reply_to_id: platform_reply_to_id,
                        external_id: Some(&sent_msg.id.0.to_string()),
                        sent_at: Some(
                            jiff::Timestamp::from_second(sent_msg.date.timestamp())?.into(),
                        ),
                    })
                    .await?;

                self.ctx.agent.group_trigger.on_agent_replied(chat_id);
            }
            BotSignal::SendVoice {
                chat_id,
                audio_bytes,
                prompt,
                platform_reply_to_id,
                ..
            } => {
                tracing::info!(
                    chat_id = chat_id,
                    prompt = %prompt,
                    "Bot sent voice"
                );

                let platform_chat_id = self.resolve_platform_chat_id(chat_id).await?;
                let services = &self.ctx.db.srv;
                let bot = &self.ctx.bot;

                let mut send_req =
                    bot.send_voice(ChatId(platform_chat_id), InputFile::memory(audio_bytes));

                if let Some(msg_id) = platform_reply_to_id
                    && let Some(msg) = services.message.get_message_by_id(msg_id).await?
                    && let Some(id) = msg.external_id.map(|id| id.parse::<i32>()).transpose()?
                {
                    send_req = send_req.reply_parameters(ReplyParameters::new(MessageId(id)));
                }

                let sent_msg = send_req.await?;

                let model = self.ctx.agent.current_model();
                let bot_account_id = self.ctx.bot.account_id();
                let file_id = sent_msg
                    .voice()
                    .map(|v| v.file.id.clone())
                    .unwrap_or_else(|| teloxide::types::FileId(format!("tts_{}", Uuid::now_v7())));
                let content_value = serde_json::to_value(vec![TelegramContentPart::Voice {
                    attachment_id: Uuid::now_v7(),
                    file_id,
                    meta: Some(VoiceMeta { prompt }),
                }])?;

                services
                    .message
                    .save_agent_message(NewAgentMessage {
                        chat_id,
                        account_id: Some(bot_account_id),
                        content: content_value,
                        model: &model,
                        tokens: 0,
                        reply_to_id: platform_reply_to_id,
                        external_id: Some(&sent_msg.id.0.to_string()),
                        sent_at: Some(
                            jiff::Timestamp::from_second(sent_msg.date.timestamp())?.into(),
                        ),
                    })
                    .await?;

                self.ctx.agent.group_trigger.on_agent_replied(chat_id);
            }
        }

        Ok(())
    }

    async fn resolve_platform_chat_id(&self, chat_id: i64) -> Result<i64> {
        self.ctx
            .db
            .srv
            .platform
            .get_chat_by_id(chat_id)
            .await?
            .ok_or_err_msg(
                ErrorKind::NotFound,
                format!("Chat not found for internal id: {}", chat_id),
            )
            .and_then(|chat| chat.external_id.parse::<i64>().map_err(Into::into))
    }
}
