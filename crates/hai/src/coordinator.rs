use anyhow::Result;
use std::sync::Arc;
use teloxide::Bot;
use tokio::sync::mpsc;

use crate::{
    agent::{AgentHandler, multimodal::MultimodalService, openrouter::RawOpenRouterClient},
    config::{AppConfig, Config},
    app::{db, service::ServiceContext},
    platform::telegram::{BotHandler, TelegramSender},
    trigger::{AgentEvent, BotSignal, GroupTrigger},
};

pub struct Coordinator {
    pub config_mgr: Config<AppConfig>,
}

impl Coordinator {
    pub fn new(config_mgr: Config<AppConfig>) -> Self {
        Self { config_mgr }
    }

    pub async fn run(self) -> Result<()> {
        let config = self.config_mgr.load();

        let pool = db::init_pool(&config.database).await?;
        let bot = Bot::new(&config.telegram.bot_token);

        let openrouter_client = RawOpenRouterClient::new(&config.agent.api_key);
        let multimodal = MultimodalService::new(&openrouter_client);
        let services = Arc::new(ServiceContext::new(
            pool.clone(),
            Arc::clone(&multimodal.embedding),
        ));
        let bot_account_id = services.platform.ensure_bot_account().await?.id;

        let (agent_event_tx, agent_event_rx) = mpsc::unbounded_channel::<AgentEvent>();
        let (bot_signal_tx, bot_signal_rx) = mpsc::unbounded_channel::<BotSignal>();

        let agent_handler = Arc::new(
            AgentHandler::new(
                &config,
                pool,
                multimodal,
                Arc::clone(&services),
                bot_signal_tx,
                bot_account_id,
            )
            .await?,
        );

        let group_trigger = Arc::new(GroupTrigger::new());

        let sender = Arc::new(TelegramSender::new(
            bot.clone(),
            Arc::clone(&services.platform),
            Arc::clone(&services.message),
            Arc::clone(&group_trigger),
        ));

        let bot_handler = Arc::new(
            BotHandler::new(
                &config,
                bot,
                Arc::clone(&agent_handler),
                agent_event_tx,
                services,
                group_trigger,
            )
            .await?,
        );

        let current_model = agent_handler.current_model();

        tokio::try_join!(
            agent_handler.run(agent_event_rx),
            sender.run(bot_signal_rx, current_model, bot_account_id),
            bot_handler.run(),
        )?;

        Ok(())
    }
}
