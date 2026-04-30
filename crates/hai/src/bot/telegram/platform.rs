use std::sync::Arc;

use tap::Tap;
use teloxide::{
    Bot,
    dispatching::dialogue::InMemStorage,
    dptree,
    prelude::*,
    types::{Me, Message},
    utils::command::BotCommands,
};

use super::util::{ExtractedTelegramMessage, is_mentioning_user, msg_chat_type};
use crate::{
    agent::{
        event::{AgentEvent, TriggerCause, TriggerStatus},
        link::{BotId, BotLink},
    },
    app::AppContext,
    domain::{
        entity::{Account, Chat, ChatType, Platform},
        vo::{PlatformAccountMeta, TelegramAccountMeta},
    },
    error::{AppError, AppResultExt, ErrorKind, Result},
};

const MAJOR_HELP_TEXT: &str = r#""#;

#[derive(Debug, BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
pub enum Command {
    #[command(description = "Start")]
    Start,
    #[command(aliases = ["h", "?"], description = "显示帮助信息")]
    Help,
    #[command(description = "当前会话状态")]
    Status,
    #[command(description = "整理记忆")]
    OrganizeMemory,
}

#[derive(Debug, Clone, Default)]
pub enum State {
    #[default]
    Start,
}

/// Telegram 平台适配器
///
/// 负责接收 Telegram 更新并将事件转发给 agent。
/// 发送消息由 `TelegramBotActor` 负责，platform 不再持有 signal channel。
pub struct TelegramPlatform {
    pub bot_id: BotId,
    pub bot: Bot,
    pub ctx: AppContext,
    pub link: BotLink,
    pub allowed_chat_ids: Vec<i64>,
}

impl TelegramPlatform {
    pub async fn new(
        bot_id: BotId,
        bot: Bot,
        ctx: AppContext,
        link: BotLink,
        allowed_chat_ids: Vec<i64>,
    ) -> Result<Self> {
        bot.set_my_commands(Command::bot_commands()).await?;
        Ok(Self {
            bot_id,
            bot,
            ctx,
            link,
            allowed_chat_ids,
        })
    }

    pub async fn run(self) -> Result<()> {
        Self::run_dispatcher(
            self.bot_id,
            self.bot,
            self.ctx,
            self.link.event_tx,
            self.allowed_chat_ids,
        )
        .await
    }

    async fn run_dispatcher(
        bot_id: BotId,
        bot: Bot,
        ctx: AppContext,
        event_tx: tokio::sync::mpsc::UnboundedSender<AgentEvent>,
        allowed_chat_ids: Vec<i64>,
    ) -> Result<()> {
        let handler = Arc::new(DispatchCtx {
            ctx,
            event_tx,
            allowed_chat_ids,
        });

        let dispatcher_handler = Update::filter_message()
            .branch(
                dptree::entry()
                    .filter(|msg: Message, h: Arc<DispatchCtx>| {
                        if !h.is_allowed_chat(msg.chat.id) {
                            tracing::warn!(
                                "Unauthorized message attempt from chat_id: {}, user: {:?}",
                                msg.chat.id,
                                msg.from.as_ref().map(|u| &u.username)
                            );
                            return false;
                        }
                        true
                    })
                    .branch(dptree::entry().filter_command::<Command>().endpoint(
                        |bot: Bot, msg: Message, cmd: Command, h: Arc<DispatchCtx>| async move {
                            tokio::spawn(DispatchCtx::handle_command(bot, msg, cmd, h));
                            Ok::<(), AppError>(())
                        },
                    ))
                    .endpoint(|msg: Message, h: Arc<DispatchCtx>, me: Me| async move {
                        let _ = h.handle_message(msg, me).await;
                        Ok::<(), AppError>(())
                    }),
            )
            .branch(dptree::entry().enter_dialogue::<Message, InMemStorage<State>, State>())
            .endpoint(|_: Bot, _: Message, _: Arc<DispatchCtx>| async { Ok(()) });

        Dispatcher::builder(bot, dispatcher_handler)
            .dependencies(dptree::deps![handler, InMemStorage::<State>::new()])
            .enable_ctrlc_handler()
            .build()
            .dispatch()
            .tap(|_| tracing::info!(bot_id = %bot_id, "Telegram dispatcher started"))
            .await;

        Ok(())
    }
}

// ─── 内部 dispatcher handler ──────────────────────────────────────────────────

struct DispatchCtx {
    ctx: AppContext,
    event_tx: tokio::sync::mpsc::UnboundedSender<AgentEvent>,
    allowed_chat_ids: Vec<i64>,
}

impl DispatchCtx {
    async fn handle_message(&self, msg: Message, me: Me) -> Result<()> {
        let Some(from) = msg.from.as_ref() else {
            return Ok(());
        };
        let chat_type = msg_chat_type(&msg);

        let (chat, account) = self.resolve_chat_and_account(&msg, from, chat_type).await?;
        self.persist_user_message(&msg, chat.id, account.id).await?;
        self.dispatch_agent_event(chat.id, chat_type, &msg, &me);

        Ok(())
    }

    async fn get_internal_chat_id(&self, msg: &Message) -> Result<i64> {
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
        chat_id: i64,
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

    fn dispatch_agent_event(&self, chat_id: i64, chat_type: ChatType, msg: &Message, me: &Me) {
        if chat_type == ChatType::Private {
            let _ = self.event_tx.send(AgentEvent::Message {
                chat_id,
                cause: TriggerCause::Private,
            });
            return;
        }

        let is_mention = is_mentioning_user(msg, me.user.username.as_deref().unwrap_or(""));
        if let Some(reason) = self.ctx.agent.group_trigger.on_message(chat_id, is_mention)
            && let Err(e) = self.event_tx.send(AgentEvent::Message {
                chat_id,
                cause: reason,
            })
        {
            tracing::error!("Failed to send agent event: {}", e);
        }
    }

    fn is_allowed_chat(&self, chat_id: ChatId) -> bool {
        self.allowed_chat_ids.contains(&chat_id.0)
    }

    async fn handle_command(
        bot: Bot,
        msg: Message,
        cmd: Command,
        h: Arc<DispatchCtx>,
    ) -> Result<()> {
        let chat_id = msg.chat.id;
        match cmd {
            Command::Start => {
                bot.send_message(chat_id, "Hello!").await?;
            }
            Command::Help => {
                bot.send_message(
                    chat_id,
                    format!("{}{MAJOR_HELP_TEXT}", Command::descriptions()),
                )
                .await?;
            }
            Command::Status => {
                let inner_chat_id = h.get_internal_chat_id(&msg).await?;
                let s = h.ctx.agent.group_trigger.status(inner_chat_id);
                let text = format_trigger_status(&s);
                bot.send_message(chat_id, text).await?;
            }
            Command::OrganizeMemory => {
                let inner_chat_id = h.get_internal_chat_id(&msg).await?;
                h.event_tx
                    .send(AgentEvent::Message {
                        chat_id: inner_chat_id,
                        cause: TriggerCause::Command(
                            "执行记忆/主题整理, 包括不限于处理不符合规范的记忆或主题, 删除重建"
                                .into(),
                        ),
                    })
                    .err_kind(ErrorKind::Internal)?;
            }
        }
        Ok(())
    }
}

fn format_trigger_status(s: &TriggerStatus) -> String {
    let window_status = if s.is_in_window {
        format!("✅ 窗口期内 (剩余: {:.0} 秒)", s.window_remaining_secs)
    } else {
        "⚪ 窗口期外".to_string()
    };

    format!(
        "📊 Agent 触发状态\n\
         \n\
         窗口状态: {}\n\
         当前热度: {} {:.1}%",
        window_status,
        render_bar(s.heat),
        s.heat * 100.0,
    )
}

fn render_bar(value: f64) -> String {
    let filled = (value.clamp(0.0, 1.0) * 10.0).round() as usize;
    let empty = 10usize.saturating_sub(filled);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}
