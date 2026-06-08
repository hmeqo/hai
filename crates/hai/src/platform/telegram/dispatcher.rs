use std::sync::Arc;

use kameo::actor::ActorRef;
use tap::Tap;
use teloxide::{
    Bot,
    dispatching::{HandlerExt, UpdateFilterExt, dialogue::InMemStorage},
    dptree,
    prelude::*,
    types::{Me, Message, Update},
    utils::command::BotCommands,
};

use super::util::{ExtractedTelegramMessage, is_mentioning_user, msg_chat_type};
use crate::{
    agent::{
        event::{WakeEvent, WakeReason},
        link::{BotHandle, BotId},
        runtime::{
            actor::{ChatActor, GetStatus},
            registry::ChatActorManager,
        },
    },
    app::AppContext,
    domain::{
        entity::{Account, Chat, ChatType, Platform},
        vo::{ChatId, PlatformAccountMeta, TelegramAccountMeta},
    },
    error::{AppError, AppResultExt, ErrorKind, Result},
    ext::kameo::KameoExt,
};

const MAJOR_HELP_TEXT: &str = r#"
你现在正与一位 AI 助手（Agent）对话。这个 Agent 拥有以下能力：
- 借助多模态理解并分析图片、视频、音频等附件
- 管理对话历史并持续累积记忆
- 自主识别和推进话题
- 向您请教和学习
- 随时随地请求总结或梳理讨论
- 可在 Telegram 上使用
"#;

#[derive(Debug, BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
pub(crate) enum Command {
    #[command(description = "启动机器人")]
    Start,
    #[command(description = "获取帮助")]
    Help,
    #[command(description = "查看 Agent 状态")]
    Status,
    #[command(description = "整理记忆和主题")]
    OrganizeMemory,
}

/// Telegram 分发器
pub struct TelegramDispather {
    pub bot_id: BotId,
    pub bot: Bot,
    pub ctx: AppContext,
    pub handle: BotHandle,
    pub registry: ChatActorManager,
    pub allowed_chat_ids: Vec<i64>,
}

impl TelegramDispather {
    pub async fn new(
        bot_id: BotId,
        bot: Bot,
        ctx: AppContext,
        registry: ChatActorManager,
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
                                    tokio::spawn(async move {
                                        if let Err(err) = dp.handle_command(bot, msg, cmd).await {
                                            tracing::error!("Failed to handle command: {}", err);
                                        }
                                    });
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

    async fn actor(&self, chat_id: ChatId) -> ActorRef<ChatActor> {
        self.registry.get_or_create(chat_id).await
    }

    async fn handle_message(&self, msg: Message, me: Me) -> Result<()> {
        let Some(from) = msg.from.as_ref() else {
            return Ok(());
        };
        let chat_type = msg_chat_type(&msg);

        let (chat, account) = self.resolve_chat_and_account(&msg, from, chat_type).await?;
        self.persist_user_message(&msg, chat.id, account.id).await?;
        self.dispatch_agent_event(chat.id, chat_type, &msg, &me)
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
                let status_msg = match self.actor(inner_chat_id).await.ask(GetStatus).await {
                    Ok(status) => format!(
                        "📊 Agent 状态\n热度: {:.2}/{:.2}\n窗口: {}\n待处理事件: {}",
                        status.heat_value,
                        status.heat_base,
                        if status.window_active {
                            format!(
                                "活跃（{}s 后关闭）",
                                status.window_closes_in_secs.unwrap_or(0.0) as i64
                            )
                        } else {
                            "关闭".into()
                        },
                        status.pending_events,
                    ),
                    Err(_) => "📊 Agent 状态\n获取失败".into(),
                };
                self.bot.send_message(msg.chat.id, status_msg).await?;
            }
            Command::OrganizeMemory => {
                let inner_chat_id = self.get_internal_chat_id(&msg).await?;
                self.actor(inner_chat_id)
                    .await
                    .tell(WakeEvent::new(
                        inner_chat_id,
                        WakeReason::Command(
                            "执行记忆/主题整理, 包括不限于处理不符合规范的记忆或主题, 删除重建"
                                .into(),
                        ),
                    ))
                    .fire();
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
        Ok(chat.id)
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
        let reply_to_id = if let Some(reply) = msg.reply_to_message() {
            self.ctx
                .db
                .srv
                .message
                .find_id_by_external_id(chat_id, &reply.id.0.to_string())
                .await?
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
                external_id: &msg.id.0.to_string(),
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
        self.actor(chat_id)
            .await
            .tell(WakeEvent::new(chat_id, reason))
            .fire();
    }
}

// ─── 状态 ──────────────────────────────────────────────────────────────────────

#[derive(Clone, Default)]
pub(crate) enum State {
    #[default]
    Start,
}
