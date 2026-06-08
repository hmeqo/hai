use kameo::actor::ActorRef;
use tokio::time::Duration;

use super::{
    AgentEngine,
    ctx::RoundCtx,
    event::{
        WakeEvent,
        scheduler::{DispatchResult, EventScheduler},
    },
    session::TaskSession,
};
use crate::{
    agent::{event::scheduler::SchedulerStatus, link::BotHandle, round::RoundResult},
    config::schema::SessionConfig,
    domain::{entity::ChatType, vo::ChatId},
    ext::kameo::KameoExt,
};

pub(super) struct Orchestrator {
    engine: AgentEngine,
    schedule: EventScheduler,
    session: Option<TaskSession>,
    idle_timeout: Duration,
    bot: BotHandle,
    chat_id: ChatId,
    chat_type: ChatType,
    notifier: ActorRef<super::actor::ChatActor>,
}

impl Orchestrator {
    pub fn new(
        engine: AgentEngine,
        schedule: EventScheduler,
        idle_timeout: Duration,
        bot: BotHandle,
        chat_id: ChatId,
        chat_type: ChatType,
        notifier: ActorRef<super::actor::ChatActor>,
    ) -> Self {
        Self {
            engine,
            schedule,
            session: None,
            idle_timeout,
            bot,
            chat_id,
            chat_type,
            notifier,
        }
    }

    pub async fn on_wake(&mut self, event: WakeEvent) {
        self.schedule.push(event);

        if !self.session.as_ref().is_some_and(|s| s.is_task_active()) {
            self.try_dispatch().await;
        }
    }

    pub fn on_result(&mut self, result: RoundResult) {
        match result {
            RoundResult::Completed(round) => {
                if let Some(session) = &mut self.session {
                    session.push(round);
                    session.clear_task();
                }
                if matches!(
                    self.engine.app.cfg.agent.context.session,
                    SessionConfig::SingleRound
                ) {
                    self.session = None;
                }
            }
            RoundResult::Failed => {
                if let Some(session) = &mut self.session {
                    session.clear_task();
                }
            }
        }
    }

    pub fn status(&mut self) -> SchedulerStatus {
        self.schedule.snapshot()
    }

    pub fn refresh_window(&mut self) {
        self.schedule.refresh();
    }

    fn get_or_create(&mut self) -> &mut TaskSession {
        self.session.get_or_insert_with(TaskSession::new)
    }

    async fn try_dispatch(&mut self) {
        if self.schedule.is_expired(self.idle_timeout) {
            self.session = None;
            return;
        }

        let DispatchResult::Ready(events) = self.schedule.try_dispatch() else {
            return;
        };

        self.spawn_round(events).await;
    }

    async fn spawn_round(&mut self, events: Vec<WakeEvent>) {
        let ctx = self.build_ctx(events);
        let messages = self.gather_messages().await;
        let notifier = self.notifier.clone();
        let engine = self.engine.clone();
        self.get_or_create()
            .spawn(engine, ctx, messages, move |result| {
                notifier.tell(result).fire();
            })
            .await;
    }

    fn build_ctx(&self, events: Vec<WakeEvent>) -> RoundCtx {
        RoundCtx {
            app: self.engine.app.clone(),
            chat_id: self.chat_id,
            chat_type: self.chat_type,
            bot: self.bot.clone(),
            events,
            skill_manager: self.engine.skill_manager.clone(),
            session: self.notifier.clone(),
        }
    }

    async fn gather_messages(&self) -> Vec<crate::domain::entity::Message> {
        let min_count = if self.session.as_ref().is_some_and(|s| !s.is_empty()) {
            0
        } else {
            self.engine.app.cfg.agent.context.message_history_limit
        };
        match self
            .engine
            .app
            .db
            .srv
            .message
            .get_messages(self.chat_id, min_count)
            .await
        {
            Ok((msgs, _)) => msgs,
            Err(e) => {
                tracing::error!(?self.chat_id, "Failed to gather messages: {e}");
                Vec::new()
            }
        }
    }
}
