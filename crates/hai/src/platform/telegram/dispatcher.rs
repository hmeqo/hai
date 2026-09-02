use std::sync::Arc;

use teloxide::{
    Bot,
    dispatching::{HandlerExt, UpdateFilterExt},
    dptree,
    prelude::*,
    types::{Me, Message, Update},
    utils::command::BotCommands,
};

use super::{command::Command, message_handler::MessageHandler, util::msg_chat_type};
use crate::{
    agent::{
        event::{AgentCommand, WakeEvent, WakeReason},
        runtime::registry::SessionManager,
    },
    app::AppContext,
    domain::vo::ChatId,
    error::{AppError, Result},
};

pub struct TelegramDispatcher {
    bot: Bot,
    allowed_chat_ids: Vec<i64>,
    msg_handler: MessageHandler,
}

impl TelegramDispatcher {
    pub async fn new(
        bot: Bot,
        ctx: AppContext,
        registry: SessionManager,
        allowed_chat_ids: Vec<i64>,
    ) -> Result<Self> {
        bot.set_my_commands(Command::bot_commands()).await?;
        Ok(Self {
            bot,
            allowed_chat_ids,
            msg_handler: MessageHandler::new(ctx, registry),
        })
    }

    pub async fn run(self) -> Result<()> {
        let this = Arc::new(self);

        let dispatcher_handler = Update::filter_message().branch(
            dptree::entry()
                .filter(|bot: Bot, msg: Message, dp: Arc<TelegramDispatcher>| {
                    if !dp.is_allowed_chat(msg.chat.id) {
                        tokio::spawn(async move {
                            if let Err(err) = dp.handle_unauthorized_message(bot, msg).await {
                                tracing::error!("Failed to handle unauthorized message: {}", err);
                            }
                            Ok::<(), AppError>(())
                        });
                        return false;
                    }
                    true
                })
                .branch(dptree::entry().filter_command::<Command>().endpoint(
                    |bot: Bot, msg: Message, cmd: Command, dp: Arc<TelegramDispatcher>| async move {
                        if let Err(err) = dp.handle_command(bot, msg, cmd).await {
                            tracing::error!("Failed to handle command: {err}");
                        }
                        Ok::<(), AppError>(())
                    },
                ))
                .endpoint(
                    |msg: Message, me: Me, dp: Arc<TelegramDispatcher>| async move {
                        if let Err(err) = dp.handle_message(msg, me).await {
                            tracing::error!("Failed to handle message: {}", err);
                        }
                        Ok::<(), AppError>(())
                    },
                ),
        );

        Dispatcher::builder(this.bot.clone(), dispatcher_handler)
            .dependencies(dptree::deps![Arc::clone(&this)])
            .enable_ctrlc_handler()
            .build()
            .dispatch()
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

    async fn handle_message(&self, msg: Message, me: Me) -> Result<()> {
        let Some(from) = msg.from.as_ref() else {
            return Ok(());
        };
        tracing::debug!(
            chat_id = %msg.chat.id,
            from = %from.full_name(),
            "Message received",
        );
        let chat_type = msg_chat_type(&msg);

        let (chat, account) = self
            .msg_handler
            .resolve_chat_and_account(&msg, from, chat_type)
            .await?;
        self.msg_handler
            .persist_user_message(&msg, ChatId::from(chat.id), account.id)
            .await?;
        self.msg_handler
            .dispatch_agent_event(ChatId::from(chat.id), chat_type, &msg, &me)
            .await;

        Ok(())
    }

    async fn handle_command(&self, _bot: Bot, msg: Message, cmd: Command) -> Result<()> {
        // 命令输入与普通消息同路径落库（agent 上下文可见用户打了什么命令）
        let chat_type = msg_chat_type(&msg);
        let Some(from) = msg.from.as_ref() else {
            tracing::warn!("Command without sender: {:?}", msg.id);
            return Ok(());
        };
        let (chat, account) = self
            .msg_handler
            .resolve_chat_and_account(&msg, from, chat_type)
            .await?;
        self.msg_handler
            .persist_user_message(&msg, ChatId::from(chat.id), account.id)
            .await?;

        match cmd {
            Command::Start => {
                self.bot.send_message(msg.chat.id, "Hello!").await?;
            }
            Command::Status => {
                let inner_chat_id = ChatId::from(chat.id);
                let status_msg = match self
                    .msg_handler
                    .session(inner_chat_id)
                    .await?
                    .status()
                    .await
                {
                    Some(s) => {
                        let sched = &s.scheduler;
                        let mut lines = vec![format!("model   {}", s.model)];

                        let runs_line = if let Some(secs) = s.turn_elapsed_secs {
                            format!("steps   {} · active {:.1}s", s.step_count, secs)
                        } else {
                            format!("steps   {}", s.step_count)
                        };
                        lines.push(runs_line);

                        if s.context_tokens > 0 {
                            lines.push(format!("tokens  {}", fmt_tokens(s.context_tokens)));
                        }

                        lines.push(format!("conv    {} msgs", s.conversation_msgs));

                        let heat =
                            format!("heat    {:.2} / {:.2}", sched.heat_value, sched.heat_base);
                        let heat_line = if sched.window_active {
                            let secs = sched.window_closes_in_secs.unwrap_or(0.0) as i64;
                            format!("{} · window {}s", heat, secs)
                        } else {
                            heat
                        };
                        lines.push(heat_line);

                        lines.join("\n")
                    }
                    None => "model   (no session)".into(),
                };
                self.bot.send_message(msg.chat.id, status_msg).await?;
            }
            Command::OrganizeMemory => {
                let inner_chat_id = ChatId::from(chat.id);
                self.msg_handler
                    .session(inner_chat_id)
                    .await?
                    .wake(WakeEvent::new(WakeReason::Command(
                        AgentCommand::OrganizeMemory,
                    )));
            }
            Command::Explain => {
                let inner_chat_id = ChatId::from(chat.id);
                self.msg_handler
                    .session(inner_chat_id)
                    .await?
                    .wake(WakeEvent::new(WakeReason::Command(AgentCommand::Explain)));
            }
            Command::Digest(days) => {
                let inner_chat_id = ChatId::from(chat.id);
                let days = days.max(1);
                self.msg_handler
                    .session(inner_chat_id)
                    .await?
                    .wake(WakeEvent::new(WakeReason::Command(AgentCommand::Digest(
                        days,
                    ))));
            }
        }
        Ok(())
    }
}

fn fmt_tokens(n: u32) -> String {
    if n >= 1000 {
        format!("{}.{}k", n / 1000, (n % 1000) / 100)
    } else {
        n.to_string()
    }
}
