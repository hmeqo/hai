use std::sync::Arc;

use teloxide::Bot;
use tokio::task::JoinHandle;

use super::{TelegramDispatcher, TelegramPlatformHandler};
use crate::{
    agent::runtime::{AgentEngine, registry::SessionManager},
    app::AppContext,
    config::schema::BotConfig,
    error::Result,
};

pub struct TelegramPlatform;

impl TelegramPlatform {
    pub async fn spawn(
        cfg: &BotConfig,
        ctx: &AppContext,
        engine: &AgentEngine,
    ) -> Result<JoinHandle<()>> {
        let bot = Bot::new(&cfg.bot_token);
        let handler = Arc::new(TelegramPlatformHandler::new(bot.clone(), ctx.clone(), cfg).await?);
        let registry = SessionManager::new(handler, engine.clone());
        let dispatcher =
            TelegramDispatcher::new(bot, ctx.clone(), registry, cfg.allowed_chat_ids.clone())
                .await?;
        let bot_label = cfg.key.clone();
        Ok(tokio::spawn(async move {
            tracing::info!(bot = %bot_label, "dispatcher started");
            if let Err(e) = dispatcher.run().await {
                tracing::error!(bot = %bot_label, "dispatcher stopped: {e}");
            } else {
                tracing::info!(bot = %bot_label, "dispatcher stopped");
            }
        }))
    }
}
