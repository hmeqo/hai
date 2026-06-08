use std::sync::Arc;

use teloxide::{Bot, prelude::Requester};

use crate::{
    agent::{
        link::{BotHandle, BotId, BotProfile},
        runtime::{AgentEngine, registry::ChatActorManager},
    },
    app::AppContext,
    config::schema::{BotConfig, BotPlatform},
    error::Result,
    platform::telegram::{TelegramDispather, TelegramPlatformHandler},
};

/// 从配置启动所有 bot 实例
pub async fn spawn_bots(ctx: &AppContext, engine: &AgentEngine) -> Result<()> {
    for (key, raw_cfg) in &ctx.cfg.bot {
        let resolved = BotConfig::resolve(key, raw_cfg)?;
        let bot_id = BotId::new(key.clone());

        match resolved.platform {
            BotPlatform::Telegram => {
                let bot = Bot::new(&resolved.bot_token);

                let bot_account = ctx.db.srv.platform.ensure_bot_account().await?;
                let me = bot.get_me().await?;
                let my_name = bot.get_my_name().await?;

                let profile = BotProfile {
                    account_id: bot_account.id,
                    username: me.username.clone().unwrap_or_default(),
                    name: my_name.name,
                };

                let handler =
                    TelegramPlatformHandler::new(bot.clone(), bot_account.id, ctx.clone());
                let handle = BotHandle::new(bot_id.clone(), profile, Arc::new(handler));

                let dispatcher = TelegramDispather::new(
                    bot_id.clone(),
                    bot,
                    ctx.clone(),
                    ChatActorManager::new(handle.clone(), engine.clone()),
                    handle,
                    resolved.allowed_chat_ids,
                )
                .await?;

                tokio::spawn(async move { dispatcher.run().await });
            }
        }
    }

    Ok(())
}
