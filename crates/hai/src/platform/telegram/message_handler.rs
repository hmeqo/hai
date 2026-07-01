use teloxide::types::{Me, Message};

use super::util::{ExtractedTelegramMessage, is_mentioning_user, msg_chat_type};
use crate::{
    agent::{
        event::{WakeEvent, WakeReason},
        runtime::{ChatSessionHandle, registry::ChatSessionManager},
    },
    app::AppContext,
    domain::{
        model::{Account, Chat, ChatType, Platform},
        vo::{ChatId, PlatformAccountMeta, TelegramAccountMeta},
    },
    error::{AppResultExt, ErrorKind, Result},
};

/// 消息处理层：账号解析、消息持久化、Agent 事件分发。
pub(super) struct MessageHandler {
    ctx: AppContext,
    registry: ChatSessionManager,
}

impl MessageHandler {
    pub fn new(ctx: AppContext, registry: ChatSessionManager) -> Self {
        Self { ctx, registry }
    }

    pub(super) async fn session(&self, chat_id: ChatId) -> ChatSessionHandle {
        self.registry.get_or_create(chat_id).await
    }

    pub(super) async fn get_internal_chat_id(&self, msg: &Message) -> Result<ChatId> {
        let Some(from) = msg.from.as_ref() else {
            return Err(ErrorKind::BadRequest.msg("No sender"));
        };
        let (chat, _) = self
            .resolve_chat_and_account(msg, from, msg_chat_type(msg))
            .await?;
        Ok(ChatId::from(chat.id))
    }

    pub(super) async fn resolve_chat_and_account(
        &self,
        msg: &Message,
        from: &teloxide::types::User,
        chat_type: ChatType,
    ) -> Result<(Chat, Account)> {
        let account_meta = PlatformAccountMeta::Telegram(TelegramAccountMeta {
            first_name: from.first_name.clone(),
            last_name: from.last_name.clone(),
            username: from.username.clone(),
        });
        self.ctx
            .db
            .srv
            .platform
            .ensure_chat_and_account(
                Platform::Telegram,
                &msg.chat.id.to_string(),
                chat_type,
                msg.chat.title(),
                &from.id.to_string(),
                Some(serde_json::to_value(account_meta)?),
            )
            .await
            .err_kind(ErrorKind::Internal)
    }

    pub(super) async fn persist_user_message(
        &self,
        msg: &Message,
        chat_id: ChatId,
        account_id: i64,
    ) -> Result<()> {
        let reply_to_id: Option<i64> = if let Some(reply) = msg.reply_to_message() {
            self.ctx
                .db
                .srv
                .message
                .get_message_id_by_external_id(chat_id, &reply.id.0.to_string())
                .await?
                .map(|id| id.0)
        } else {
            None
        };

        let extracted = ExtractedTelegramMessage::extract(msg);
        self.ctx
            .db
            .srv
            .message
            .save_user_message(crate::domain::service::NewUserMessage {
                chat_id,
                account_id,
                content: serde_json::to_value(extracted.parts)?,
                external_id: msg.id.to_string(),
                reply_to_id,
                meta: extracted.meta,
                sent_at: Some(jiff::Timestamp::from_second(msg.date.timestamp())?.into()),
            })
            .await?;
        Ok(())
    }

    pub(super) async fn dispatch_agent_event(
        &self,
        chat_id: ChatId,
        chat_type: ChatType,
        msg: &Message,
        me: &Me,
    ) {
        let reason = if chat_type == ChatType::Private {
            WakeReason::Direct
        } else if is_mentioning_user(msg, me.user.username.as_deref().unwrap_or("")) {
            WakeReason::Mention
        } else {
            WakeReason::Observe
        };
        tracing::debug!(%chat_id, reason = reason.label(), "Agent event dispatched");
        self.session(chat_id)
            .await
            .wake(WakeEvent::new(chat_id, reason));
    }
}
