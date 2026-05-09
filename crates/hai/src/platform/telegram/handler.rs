use std::{fmt, sync::Arc};

use derive_more::Deref;
use kameo::actor::{ActorRef, Spawn};
use teloxide::Bot;
use uuid::Uuid;

use super::{
    TelegramService,
    actor::{TelegramBotActor, TypingMsg},
    media::TelegramMediaAnalyzer,
};
use crate::{
    agent::link::{PlatformHandler, SendMessageReq, SendVoiceReq, SentMessageMeta},
    app::AppContext,
    error::{AppResultExt, ErrorKind, Result},
};

// ─── Handler ──────────────────────────────────────────────────────────────────

/// 平台能力适配层：实现 `PlatformHandler`，所有业务逻辑委托给子模块。

pub struct TelegramPlatformHandlerInner {
    bot_actor: ActorRef<TelegramBotActor>,
    media: TelegramMediaAnalyzer,
}

#[derive(Deref)]
pub struct TelegramPlatformHandler(Arc<TelegramPlatformHandlerInner>);

impl TelegramPlatformHandler {
    fn new(bot_actor: ActorRef<TelegramBotActor>, media: TelegramMediaAnalyzer) -> Self {
        Self(Arc::new(TelegramPlatformHandlerInner { bot_actor, media }))
    }
}

impl fmt::Debug for TelegramPlatformHandler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelegramPlatformHandler")
            .field("actor_ref", &self.bot_actor)
            .finish()
    }
}

#[async_trait::async_trait]
impl PlatformHandler for TelegramPlatformHandler {
    async fn send_message(&self, req: SendMessageReq) -> Result<SentMessageMeta> {
        Ok(self
            .bot_actor
            .ask(req)
            .await
            .err_kind_msg(ErrorKind::Internal, "TelegramBotActor mailbox error")?)
    }

    async fn send_voice(&self, req: SendVoiceReq) -> Result<SentMessageMeta> {
        Ok(self
            .bot_actor
            .ask(req)
            .await
            .err_kind_msg(ErrorKind::Internal, "TelegramBotActor mailbox error")?)
    }

    async fn send_typing(&self, chat_id: i64) {
        if let Err(err) = self.bot_actor.tell(TypingMsg { chat_id }).send().await {
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
}

// ─── 工厂 ─────────────────────────────────────────────────────────────────────

/// 启动 `TelegramBotActor`，返回可共享的 `PlatformHandler`
pub fn spawn_telegram_handler(
    bot: Bot,
    account_id: i64,
    ctx: AppContext,
) -> Arc<TelegramPlatformHandler> {
    let bot_actor =
        TelegramBotActor::spawn(TelegramBotActor::new(bot.clone(), account_id, ctx.clone()));
    Arc::new(TelegramPlatformHandler::new(
        bot_actor,
        TelegramMediaAnalyzer::new(TelegramService::new(bot), ctx),
    ))
}
