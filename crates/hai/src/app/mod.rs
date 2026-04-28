pub mod context;

use std::sync::Arc;

pub use context::AppContext;
use tokio::sync::mpsc;

use crate::{
    agent::{
        AgentHandler,
        event::{AgentEvent, BotSignal},
    },
    bot::telegram::{BotHandler, BotSignalHandler},
    config::AppConfigManager,
    error::Result,
};

pub struct App {
    pub config_mgr: AppConfigManager,
}

impl App {
    pub fn new(config_mgr: AppConfigManager) -> Self {
        Self { config_mgr }
    }

    pub async fn serve(config_mgr: AppConfigManager) -> Result<()> {
        let cfg = config_mgr.load();

        tracing_subscriber::FmtSubscriber::builder()
            .with_max_level(cfg.logging.level())
            .init();

        App::new(config_mgr).run().await
    }

    pub async fn run(self) -> Result<()> {
        let ctx = AppContext::new(self.config_mgr).await?;

        let (agent_event_tx, agent_event_rx) = mpsc::unbounded_channel::<AgentEvent>();
        let (bot_signal_tx, bot_signal_rx) = mpsc::unbounded_channel::<BotSignal>();

        let agent_handler = Arc::new(AgentHandler::new(ctx.clone(), bot_signal_tx).await?);
        let bot_signal_handler = Arc::new(BotSignalHandler::new(ctx.clone()));
        let bot_handler = Arc::new(BotHandler::new(ctx, agent_event_tx).await?);

        tokio::spawn(async move {
            let _ = agent_handler.run(agent_event_rx).await;
        });

        tokio::spawn(async move {
            let _ = bot_signal_handler.run(bot_signal_rx).await;
        });

        bot_handler.run().await
    }
}
