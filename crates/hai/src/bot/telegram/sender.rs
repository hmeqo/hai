use std::sync::Arc;

use kameo::actor::{ActorRef, Spawn};
use teloxide::Bot;

use super::actor::{TelegramBotActor, TypingMsg};
use crate::{
    agent::link::{BotSender, SendMessageReq, SendVoiceReq, SentMessageMeta},
    app::AppContext,
    error::{AppResultExt, ErrorKind, Result},
};

// ─── BotSender 实现 ───────────────────────────────────────────────────────────

/// kameo ActorRef 的包装，实现 `BotSender`，供 `BotConn` 持有
pub struct TelegramSender(ActorRef<TelegramBotActor>);

#[async_trait::async_trait]
impl BotSender for TelegramSender {
    async fn send_message(&self, req: SendMessageReq) -> Result<SentMessageMeta> {
        Ok(self
            .0
            .ask(req)
            .await
            .err_kind_msg(ErrorKind::Internal, "TelegramBotActor mailbox error")?)
    }

    async fn send_voice(&self, req: SendVoiceReq) -> Result<SentMessageMeta> {
        Ok(self
            .0
            .ask(req)
            .await
            .err_kind_msg(ErrorKind::Internal, "TelegramBotActor mailbox error")?)
    }

    fn send_typing(&self, chat_id: i64) {
        let _ = self.0.tell(TypingMsg { chat_id });
    }
}

/// 启动 `TelegramBotActor`，返回可共享的 `BotSender`
pub fn spawn_telegram_sender(bot: Bot, account_id: i64, ctx: AppContext) -> Arc<TelegramSender> {
    let actor_ref = TelegramBotActor::spawn(TelegramBotActor::new(bot, account_id, ctx));
    Arc::new(TelegramSender(actor_ref))
}
