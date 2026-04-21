use anyhow::Result;
use std::sync::Arc;
use teloxide::Bot;
use tokio::sync::mpsc;

use crate::{
    agent::event::{AgentEvent, BotSignal, GroupTrigger},
    agent::{AgentHandler, multimodal::MultimodalService, personality::PersonalityMgr},
    config::{AppConfigManager, ProviderManager},
    domain::{db, service::Services},
    infra::platform::telegram::{BotHandler, BotIdentity, TelegramSender},
};

pub struct App {
    pub config_mgr: AppConfigManager,
}

impl App {
    pub fn new(config_mgr: AppConfigManager) -> Self {
        Self { config_mgr }
    }

    pub async fn serve(config: AppConfigManager) -> anyhow::Result<()> {
        let cfg = config.load();

        tracing_subscriber::FmtSubscriber::builder()
            .with_max_level(cfg.logging.level())
            .init();

        App::new(config).run().await?;

        Ok(())
    }

    pub async fn run(self) -> Result<()> {
        let config = self.config_mgr.load();

        let pool = db::init_pool(&config.database).await?;
        let bot = Bot::new(&config.telegram.bot_token);

        // 统一管理已解析的 provider
        let providers = ProviderManager::new(&config)?;

        // 多模态服务（未指定 provider 时使用主 agent 的 provider）
        let multimodal =
            MultimodalService::new(&providers, &config.multimodal, &config.agent.provider)?;
        let services = Arc::new(Services::new(
            pool.clone(),
            Arc::clone(&multimodal.embedding),
            Arc::clone(&config),
        ));
        let bot_account = services.platform.ensure_bot_account().await?;
        let bot_identity = BotIdentity::new(bot_account.id, &bot).await?;

        let (agent_event_tx, agent_event_rx) = mpsc::unbounded_channel::<AgentEvent>();
        let (bot_signal_tx, bot_signal_rx) = mpsc::unbounded_channel::<BotSignal>();

        let personality = PersonalityMgr::new(self.config_mgr.clone());

        let group_trigger = Arc::new(
            GroupTrigger::new()
                .with_min_heat(personality.min_heat(&config.agent.trigger))
                .with_conversation_window_secs(personality.conversation_window_secs()),
        );

        let agent_handler = Arc::new(
            AgentHandler::new(
                &config,
                pool,
                providers,
                multimodal,
                Arc::clone(&services),
                bot_signal_tx,
                personality,
                bot_identity.clone(),
            )
            .await?,
        );

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

        let current_model = agent_handler.current_model().await;

        tokio::spawn(async move {
            let _ = agent_handler.run(agent_event_rx).await;
        });

        tokio::spawn(async move {
            let _ = sender.run(bot_signal_rx, current_model, bot_identity).await;
        });

        bot_handler.run().await?;

        Ok(())
    }
}
