use kameo::{
    Actor,
    actor::ActorRef,
    message::{Context, Message},
};
use teloxide::{
    Bot,
    payloads::{SendMessageSetters, SendVoiceSetters},
    prelude::Requester,
    types::{ChatAction, ChatId, FileId, InputFile, MessageId, ReplyParameters},
};
use uuid::Uuid;

use crate::{
    agent::link::{SendMessageReq, SendVoiceReq, SentMessageMeta},
    app::AppContext,
    domain::{
        service::NewAgentMessage,
        vo::{TelegramContentPart, VoiceMeta},
    },
    error::{AppError, AppResultExt, ErrorKind, OptionAppExt, Result},
};

// ─── Actor ───────────────────────────────────────────────────────────────────

/// Telegram bot 的发送 actor
///
/// 每个 bot 实例对应一个，负责执行实际的 Telegram API 调用和消息入库。
/// 外部通过 `TelegramSender`（见 sender.rs）与之通信。
pub(crate) struct TelegramBotActor {
    bot: Bot,
    account_id: i64,
    ctx: AppContext,
}

impl TelegramBotActor {
    pub fn new(bot: Bot, account_id: i64, ctx: AppContext) -> Self {
        Self {
            bot,
            account_id,
            ctx,
        }
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
        let Some(msg) = self.ctx.db.srv.message.get_message_by_id(msg_id).await? else {
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
        chat_id: i64,
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
                model: &model,
                tokens: 0,
                reply_to_id,
                external_id: Some(external_id),
                sent_at: Some(jiff::Timestamp::from_second(sent_at_ts)?.into()),
            })
            .await?;

        self.ctx.agent.group_trigger.on_agent_replied(chat_id);
        Ok(())
    }
}

impl Actor for TelegramBotActor {
    type Args = Self;
    type Error = AppError;

    async fn on_start(
        args: Self::Args,
        _actor_ref: ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(args)
    }
}

// ─── 消息处理 ─────────────────────────────────────────────────────────────────

pub struct TypingMsg {
    pub chat_id: i64,
}

impl Message<SendMessageReq> for TelegramBotActor {
    type Reply = Result<SentMessageMeta>;

    async fn handle(
        &mut self,
        req: SendMessageReq,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let platform_chat_id = self.resolve_platform_chat_id(req.chat_id).await?;
        let reply_params = self
            .resolve_reply_parameters(req.platform_reply_to_id)
            .await?;

        let mut tg_req = self
            .bot
            .send_message(ChatId(platform_chat_id), req.content.clone());
        if let Some(params) = reply_params {
            tg_req = tg_req.reply_parameters(params);
        }
        let sent_msg = tg_req.await?;

        let content = serde_json::to_value(vec![TelegramContentPart::Text { text: req.content }])?;
        self.persist_message(
            req.chat_id,
            req.platform_reply_to_id,
            content,
            &sent_msg.id.0.to_string(),
            sent_msg.date.timestamp(),
        )
        .await?;

        Ok(SentMessageMeta {
            external_id: sent_msg.id.0.to_string(),
        })
    }
}

impl Message<SendVoiceReq> for TelegramBotActor {
    type Reply = Result<SentMessageMeta>;

    async fn handle(
        &mut self,
        req: SendVoiceReq,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let platform_chat_id = self.resolve_platform_chat_id(req.chat_id).await?;
        let reply_params = self
            .resolve_reply_parameters(req.platform_reply_to_id)
            .await?;

        let mut tg_req = self
            .bot
            .send_voice(ChatId(platform_chat_id), InputFile::memory(req.audio_bytes));
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
            &sent_msg.id.0.to_string(),
            sent_msg.date.timestamp(),
        )
        .await?;

        Ok(SentMessageMeta {
            external_id: sent_msg.id.0.to_string(),
        })
    }
}

impl Message<TypingMsg> for TelegramBotActor {
    type Reply = ();

    async fn handle(
        &mut self,
        msg: TypingMsg,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        if let Ok(platform_chat_id) = self.resolve_platform_chat_id(msg.chat_id).await {
            let _ = self
                .bot
                .send_chat_action(ChatId(platform_chat_id), ChatAction::Typing)
                .await;
        }
    }
}
