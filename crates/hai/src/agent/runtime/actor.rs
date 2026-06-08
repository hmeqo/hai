use std::path::PathBuf;

use kameo::{
    Actor,
    actor::{ActorRef, Spawn},
    message::{Context, Message},
    messages,
};
use tokio::time::Duration;

use super::{
    AgentEngine,
    event::{WakeEvent, scheduler::EventScheduler},
    orch::Orchestrator,
    shell::{ShellOutput, ShellRuntime},
};
use crate::{
    agent::{
        event::scheduler::SchedulerStatus,
        link::{BotHandle, SendMessageReq, SendVoiceReq, SentMessageMeta},
        round::RoundResult,
    },
    domain::{entity::ChatType, vo::ChatId},
    error::{AppError, AppResultExt, ErrorKind},
};

pub async fn spawn_chat_actor(
    chat_id: ChatId,
    bot: BotHandle,
    engine: AgentEngine,
) -> ActorRef<ChatActor> {
    ChatActor::spawn((chat_id, bot, engine))
}

pub struct ChatActor {
    pub chat_id: ChatId,
    pub bot: BotHandle,
    pub shell: ShellRuntime,
    orch: Orchestrator,
}

impl Actor for ChatActor {
    type Args = (ChatId, BotHandle, AgentEngine);
    type Error = AppError;

    async fn on_start(
        (chat_id, bot, engine): Self::Args,
        actor_ref: ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        let chat_type = Self::resolve_chat_type(&engine, chat_id).await?;
        let scheduler = Self::init_scheduler(&engine);
        let idle_timeout =
            Duration::from_secs(engine.app.cfg.agent.context.session_idle_timeout_secs);

        Ok(Self {
            chat_id,
            bot: bot.clone(),
            shell: ShellRuntime::new(&engine.app.cfg.sandbox),
            orch: Orchestrator::new(
                engine,
                scheduler,
                idle_timeout,
                bot,
                chat_id,
                chat_type,
                actor_ref,
            ),
        })
    }
}

impl ChatActor {
    async fn resolve_chat_type(
        engine: &AgentEngine,
        chat_id: ChatId,
    ) -> Result<ChatType, AppError> {
        Ok(engine
            .app
            .db
            .srv
            .platform
            .get_chat_by_id(chat_id)
            .await?
            .expect("Failed to get chat")
            .chat_type())
    }

    fn init_scheduler(engine: &AgentEngine) -> EventScheduler {
        let cfg = &engine.app.cfg.agent.attention;
        EventScheduler::new(
            engine.app.agent.personality.base_attention(cfg),
            engine.app.agent.personality.attention_window_secs(),
            Duration::from_millis(cfg.sustained_window_ms),
            Duration::from_millis(cfg.window_max_ms),
        )
    }
}

pub struct ExecuteShell {
    pub command: String,
    pub workdir: Option<String>,
    pub skill_dir: Option<PathBuf>,
    pub timeout_secs: Option<u64>,
}

impl Message<ExecuteShell> for ChatActor {
    type Reply = Result<ShellOutput, AppError>;

    async fn handle(
        &mut self,
        msg: ExecuteShell,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.shell
            .execute(
                &msg.command,
                msg.workdir,
                msg.skill_dir.as_deref(),
                msg.timeout_secs,
            )
            .await
            .err_kind_msg(ErrorKind::Internal, "Shell execution failed")
    }
}

impl Message<WakeEvent> for ChatActor {
    type Reply = ();

    async fn handle(
        &mut self,
        event: WakeEvent,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.orch.on_wake(event).await;
    }
}

impl Message<RoundResult> for ChatActor {
    type Reply = ();

    async fn handle(
        &mut self,
        msg: RoundResult,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.orch.on_result(msg);
    }
}

impl Message<SendMessageReq> for ChatActor {
    type Reply = Result<SentMessageMeta, AppError>;

    async fn handle(
        &mut self,
        msg: SendMessageReq,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let meta = self.bot.send_message(msg).await?;
        self.orch.refresh_window();
        Ok(meta)
    }
}

impl Message<SendVoiceReq> for ChatActor {
    type Reply = Result<SentMessageMeta, AppError>;

    async fn handle(
        &mut self,
        msg: SendVoiceReq,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let meta = self.bot.send_voice(msg).await?;
        self.orch.refresh_window();
        Ok(meta)
    }
}

#[messages]
impl ChatActor {
    #[message]
    pub fn get_status(&mut self) -> Result<SchedulerStatus, AppError> {
        Ok(self.orch.status())
    }
}
