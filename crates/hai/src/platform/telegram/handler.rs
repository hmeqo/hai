use std::{fmt, sync::Arc};

use derive_more::Deref;
use teloxide::{
    Bot,
    payloads::{SendMessageSetters, SendVoiceSetters},
    prelude::Requester,
    types::{ChatAction, FileId, InputFile, MessageId, ParseMode, ReplyParameters},
};
use uuid::Uuid;

use super::{
    TelegramService, media::TelegramMediaAnalyzer, parser::TelegramContentParser,
    util::escape_md_v2,
};
use crate::{
    agent::link::{ContentParser, PlatformHandler, SendMessageReq, SendVoiceReq, SentMessageMeta},
    app::AppContext,
    domain::{
        service::NewAgentMessage,
        vo::{ChatId, TelegramContentPart, VoiceMeta},
    },
    error::{AppResultExt, ErrorKind, OptionAppExt, Result},
};

// ─── Handler ──────────────────────────────────────────────────────────────────

pub struct TelegramPlatformHandlerInner {
    bot: Bot,
    account_id: i64,
    ctx: AppContext,
    media: TelegramMediaAnalyzer,
}

#[derive(Clone, Deref)]
pub struct TelegramPlatformHandler(Arc<TelegramPlatformHandlerInner>);

impl TelegramPlatformHandler {
    pub fn new(bot: Bot, account_id: i64, ctx: AppContext) -> Self {
        Self(Arc::new(TelegramPlatformHandlerInner {
            media: TelegramMediaAnalyzer::new(TelegramService::new(bot.clone()), ctx.clone()),
            bot,
            account_id,
            ctx,
        }))
    }

    async fn resolve_platform_chat_id(&self, chat_id: ChatId) -> Result<i64> {
        self.ctx
            .db
            .srv
            .platform
            .get_chat_by_id(chat_id)
            .await?
            .ok_or_err_msg(
                ErrorKind::NotFound,
                format!("Chat not found for internal id: {chat_id}"),
            )
            .and_then(|chat| chat.external_id.parse::<i64>().map_err(Into::into))
    }

    async fn resolve_reply_parameters(
        &self,
        platform_reply_to_id: Option<i64>,
    ) -> Result<Option<ReplyParameters>> {
        let Some(msg_id) = platform_reply_to_id else {
            return Ok(None);
        };
        let Some(msg) = self
            .ctx
            .db
            .srv
            .message
            .get_message_by_id(crate::domain::vo::MessageId(msg_id))
            .await?
        else {
            return Ok(None);
        };
        let Some(id) = msg
            .external_id
            .map(|id| id.parse::<i32>())
            .transpose()
            .err_kind(ErrorKind::DataParse)?
        else {
            return Ok(None);
        };
        Ok(Some(ReplyParameters::new(MessageId(id))))
    }

    async fn persist_message(
        &self,
        chat_id: ChatId,
        reply_to_id: Option<i64>,
        content: serde_json::Value,
        external_id: &str,
        sent_at_ts: i64,
    ) -> Result<()> {
        let model = self.ctx.agent.current_model();

        self.ctx
            .db
            .srv
            .message
            .save_agent_message(NewAgentMessage {
                chat_id,
                account_id: Some(self.account_id),
                content,
                model: model.to_string(),
                tokens: 0,
                reply_to_id,
                external_id: Some(external_id.to_string()),
                sent_at: Some(jiff::Timestamp::from_second(sent_at_ts)?.into()),
            })
            .await?;

        Ok(())
    }

    /// 先尝试 MarkdownV2（已转义），失败降级纯文本
    async fn send_with_markdown_fallback(
        &self,
        content: &str,
        cid: teloxide::types::ChatId,
        reply_params: &Option<ReplyParameters>,
    ) -> Result<teloxide::types::Message> {
        let escaped = escape_md_v2(content);
        let send_md = self.bot.send_message(cid, &escaped);
        let send_md = match reply_params {
            Some(p) => send_md.reply_parameters(p.clone()),
            None => send_md,
        };
        match send_md.parse_mode(ParseMode::MarkdownV2).await {
            Ok(msg) => Ok(msg),
            Err(_) => {
                let send_plain = self.bot.send_message(cid, content);
                let send_plain = match reply_params {
                    Some(p) => send_plain.reply_parameters(p.clone()),
                    None => send_plain,
                };
                send_plain.await.map_err(Into::into)
            }
        }
    }
}

impl fmt::Debug for TelegramPlatformHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelegramPlatformHandler").finish()
    }
}

#[async_trait::async_trait]
impl PlatformHandler for TelegramPlatformHandler {
    async fn send_message(&self, req: SendMessageReq) -> Result<SentMessageMeta> {
        let cid = teloxide::types::ChatId(self.resolve_platform_chat_id(req.chat_id).await?);
        let reply_params = self
            .resolve_reply_parameters(req.platform_reply_to_id)
            .await?;

        let sent_msg = self
            .send_with_markdown_fallback(&req.content, cid, &reply_params)
            .await?;

        let content = serde_json::to_value(vec![TelegramContentPart::Text { text: req.content }])?;
        self.persist_message(
            req.chat_id,
            req.platform_reply_to_id,
            content,
            &sent_msg.id.to_string(),
            sent_msg.date.timestamp(),
        )
        .await?;

        Ok(SentMessageMeta {
            external_id: sent_msg.id.to_string(),
        })
    }

    async fn send_voice(&self, req: SendVoiceReq) -> Result<SentMessageMeta> {
        let platform_chat_id = self.resolve_platform_chat_id(req.chat_id).await?;
        let reply_params = self
            .resolve_reply_parameters(req.platform_reply_to_id)
            .await?;

        let mut tg_req = self.bot.send_voice(
            teloxide::types::ChatId(platform_chat_id),
            InputFile::memory(req.audio_bytes),
        );
        if let Some(params) = reply_params {
            tg_req = tg_req.reply_parameters(params);
        }
        let sent_msg = tg_req.await?;

        let file_id = sent_msg
            .voice()
            .map(|v| v.file.id.clone())
            .unwrap_or_else(|| FileId(format!("tts_{}", Uuid::now_v7())));
        let content = serde_json::to_value(vec![TelegramContentPart::Voice {
            attachment_id: Uuid::now_v7(),
            file_id,
            meta: Some(VoiceMeta { prompt: req.prompt }),
        }])?;
        self.persist_message(
            req.chat_id,
            req.platform_reply_to_id,
            content,
            &sent_msg.id.to_string(),
            sent_msg.date.timestamp(),
        )
        .await?;

        Ok(SentMessageMeta {
            external_id: sent_msg.id.to_string(),
        })
    }

    async fn send_typing(&self, chat_id: ChatId) {
        if let Ok(platform_chat_id) = self.resolve_platform_chat_id(chat_id).await
            && let Err(err) = self
                .bot
                .send_chat_action(
                    teloxide::types::ChatId(platform_chat_id),
                    ChatAction::Typing,
                )
                .await
        {
            tracing::error!("Failed to send typing message: {}", err);
        }
    }

    async fn download_file(&self, file_id: &str) -> Result<Vec<u8>> {
        self.media.download_file_cached(file_id).await
    }

    async fn get_file_url(&self, file_id: &str) -> Result<String> {
        self.media.file_url(file_id).await
    }

    async fn analyze_attachment(
        &self,
        attachment_uuid: Uuid,
        prompt: Option<&str>,
    ) -> Result<String> {
        let (part, file_id, parser) = self.media.resolve_attachment(attachment_uuid).await?;
        let content = self
            .media
            .analyze_part(&part, &file_id, parser, prompt)
            .await?;
        self.media
            .persist_analysis(&file_id, parser, prompt, &content)
            .await?;
        Ok(content)
    }

    fn content_parser(&self) -> &'static dyn ContentParser {
        &TelegramContentParser
    }
}
