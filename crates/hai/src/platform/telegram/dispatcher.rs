use std::sync::Arc;

use tap::Tap;
use teloxide::{
    Bot,
    dispatching::{HandlerExt, UpdateFilterExt, dialogue::InMemStorage},
    dptree,
    prelude::*,
    types::{Me, Message, ParseMode, Update},
    utils::command::BotCommands,
};

use super::{
    command::{Command, MAJOR_HELP_TEXT},
    util::{ExtractedTelegramMessage, is_mentioning_user, msg_chat_type},
};
use crate::{
    agent::{
        event::{WakeEvent, WakeReason},
        link::{BotHandle, BotId},
        runtime::{ChatSessionHandle, registry::ChatSessionManager},
    },
    app::AppContext,
    domain::{
        model::{Account, Chat, ChatType, Platform},
        vo::{ChatId, PlatformAccountMeta, TelegramAccountMeta},
    },
    error::{AppError, AppResultExt, ErrorKind, Result},
};

/// Telegram 分发器
pub struct TelegramDispather {
    pub bot_id: BotId,
    pub bot: Bot,
    pub ctx: AppContext,
    pub handle: BotHandle,
    pub registry: ChatSessionManager,
    pub allowed_chat_ids: Vec<i64>,
}

impl TelegramDispather {
    pub async fn new(
        bot_id: BotId,
        bot: Bot,
        ctx: AppContext,
        registry: ChatSessionManager,
        handle: BotHandle,
        allowed_chat_ids: Vec<i64>,
    ) -> Result<Self> {
        bot.set_my_commands(Command::bot_commands()).await?;
        Ok(Self {
            bot_id,
            bot,
            ctx,
            registry,
            handle,
            allowed_chat_ids,
        })
    }

    pub async fn run(self) -> Result<()> {
        let this = Arc::new(self);

        let dispatcher_handler =
            Update::filter_message()
                .branch(
                    dptree::entry()
                        .filter(|bot: Bot, msg: Message, dp: Arc<TelegramDispather>| {
                            if !dp.is_allowed_chat(msg.chat.id) {
                                tokio::spawn(async move {
                                    if let Err(err) = dp.handle_unauthorized_message(bot, msg).await
                                    {
                                        tracing::error!(
                                            "Failed to handle unauthorized message: {}",
                                            err
                                        );
                                    }
                                    Ok::<(), AppError>(())
                                });
                                return false;
                            }
                            true
                        })
                        .branch(
                            dptree::entry().filter_command::<Command>().endpoint(
                                |bot: Bot,
                                 msg: Message,
                                 cmd: Command,
                                 dp: Arc<TelegramDispather>| async move {
                                    if let Err(err) = dp.handle_command(bot, msg, cmd).await {
                                        tracing::error!("Failed to handle command: {err}");
                                    }
                                    Ok::<(), AppError>(())
                                },
                            ),
                        )
                        .endpoint(
                            |msg: Message, me: Me, dp: Arc<TelegramDispather>| async move {
                                if let Err(err) = dp.handle_message(msg, me).await {
                                    tracing::error!("Failed to handle message: {}", err);
                                }
                                Ok::<(), AppError>(())
                            },
                        ),
                )
                .branch(dptree::entry().enter_dialogue::<Message, InMemStorage<State>, State>())
                .endpoint(|_: Bot, _: Message, _: Arc<TelegramDispather>| async { Ok(()) });

        Dispatcher::builder(this.bot.clone(), dispatcher_handler)
            .dependencies(dptree::deps![
                Arc::clone(&this),
                InMemStorage::<State>::new()
            ])
            .enable_ctrlc_handler()
            .build()
            .dispatch()
            .tap(|_| tracing::info!(bot_id = %this.bot_id, "Telegram dispatcher started"))
            .await;

        Ok(())
    }

    fn is_allowed_chat(&self, chat_id: teloxide::types::ChatId) -> bool {
        self.allowed_chat_ids.contains(&chat_id.0)
    }

    async fn handle_unauthorized_message(&self, _bot: Bot, msg: Message) -> Result<()> {
        tracing::warn!(
            "Unauthorized message attempt from chat_id: {}, user: {:?}",
            msg.chat.id,
            msg.from.as_ref().map(|u| &u.username)
        );
        if let Some(text) = msg.text()
            && text == "/start"
        {
            self.bot.send_message(msg.chat.id,  "You are not authorized to use this bot. If you think this is a mistake, please contact the bot owner.").await?;
        }
        Ok(())
    }

    async fn session(&self, chat_id: ChatId) -> ChatSessionHandle {
        self.registry.get_or_create(chat_id).await
    }

    async fn handle_message(&self, msg: Message, me: Me) -> Result<()> {
        let Some(from) = msg.from.as_ref() else {
            return Ok(());
        };
        tracing::info!(
            chat_id = %msg.chat.id,
            from = %from.full_name(),
            text = %msg.text().unwrap_or("<non-text>"),
            "Message received",
        );
        let chat_type = msg_chat_type(&msg);

        let (chat, account) = self.resolve_chat_and_account(&msg, from, chat_type).await?;
        self.persist_user_message(&msg, ChatId::from(chat.id), account.id)
            .await?;
        self.dispatch_agent_event(ChatId::from(chat.id), chat_type, &msg, &me)
            .await;

        Ok(())
    }

    async fn handle_command(&self, _bot: Bot, msg: Message, cmd: Command) -> Result<()> {
        match cmd {
            Command::Start => {
                self.bot.send_message(msg.chat.id, "Hello!").await?;
            }
            Command::Help => {
                self.bot
                    .send_message(
                        msg.chat.id,
                        format!("{}\n{}", Command::descriptions(), MAJOR_HELP_TEXT),
                    )
                    .await?;
            }
            Command::Status => {
                let inner_chat_id = self.get_internal_chat_id(&msg).await?;
                let status_msg = match self.session(inner_chat_id).await.status().await {
                    Some(s) => {
                        let sched = &s.scheduler;
                        let running = match s.round_elapsed_secs {
                            Some(secs) => format!("🟢 运行 `{secs:.1}s`"),
                            None => "⚪ 空闲".into(),
                        };
                        let window = if sched.window_active {
                            let secs = sched.window_closes_in_secs.unwrap_or(0.0) as i64;
                            format!("🪟 `{secs}s`")
                        } else {
                            "🪟 —".into()
                        };
                        format!(
                            "🤖 Agent · {}轮次 · `{}`\n{}\n🔥 `{:.2}` / `{:.2}`\n{}\n📥 `{}`",
                            s.rounds_completed,
                            s.model,
                            running,
                            sched.heat_value,
                            sched.heat_base,
                            window,
                            sched.pending_events,
                        )
                    }
                    None => "🤖 Agent 状态\n获取失败".into(),
                };
                self.bot
                    .send_message(msg.chat.id, status_msg)
                    .parse_mode(ParseMode::MarkdownV2)
                    .await?;
            }
            Command::OrganizeMemory => {
                let inner_chat_id = self.get_internal_chat_id(&msg).await?;
                self.session(inner_chat_id).await.wake(WakeEvent::new(
                    inner_chat_id,
                    WakeReason::Command(
                        "执行记忆/主题整理, 包括不限于处理不符合规范的记忆或主题, 删除重建".into(),
                    ),
                ));
            }
        }
        Ok(())
    }

    async fn get_internal_chat_id(&self, msg: &Message) -> Result<ChatId> {
        let Some(from) = msg.from.as_ref() else {
            return Err(ErrorKind::BadRequest.msg("No sender"));
        };
        let (chat, _) = self
            .resolve_chat_and_account(msg, from, msg_chat_type(msg))
            .await?;
        Ok(ChatId::from(chat.id))
    }

    async fn resolve_chat_and_account(
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

    async fn persist_user_message(
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

    async fn dispatch_agent_event(
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
        tracing::info!(%chat_id, reason = reason.label(), "Agent event dispatched");
        self.session(chat_id)
            .await
            .wake(WakeEvent::new(chat_id, reason));
    }
}

// ─── 状态 ──────────────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
pub(crate) enum State {
    #[default]
    Start,
}
