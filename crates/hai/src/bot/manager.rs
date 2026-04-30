use teloxide::{Bot, prelude::Requester};

use crate::{
    agent::{
        AgentGateway,
        link::{BotConn, BotId, BotProfile, open_link},
    },
    app::AppContext,
    bot::telegram::{TelegramPlatform, spawn_telegram_sender},
    config::schema::{BotConfig, BotPlatform},
    error::Result,
};

/// 从配置启动所有 bot 实例，注册到 gateway
pub async fn spawn_bots(ctx: &AppContext, gateway: &mut AgentGateway) -> Result<()> {
    for (key, raw_cfg) in &ctx.cfg.bot {
        let resolved = BotConfig::resolve(key, raw_cfg)?;
        let bot_id = BotId::new(key.clone());
        let (link, agent_link) = open_link(bot_id.clone());

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

                let sender = spawn_telegram_sender(bot.clone(), bot_account.id, ctx.clone());
                let conn = BotConn::new(bot_id.clone(), profile, sender);

                gateway.add_connection(bot_id.clone(), conn, agent_link);

                let platform = TelegramPlatform::new(
                    bot_id.clone(),
                    bot,
                    ctx.clone(),
                    link,
                    resolved.allowed_chat_ids,
                )
                .await?;

                tokio::spawn(async move {
                    if let Err(e) = platform.run().await {
                        tracing::error!(bot_id = %bot_id, "Platform error: {e}");
                    }
                });
            }
        }
    }

    Ok(())
}
