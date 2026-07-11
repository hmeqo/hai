use crate::{
    agent::runtime::AgentEngine, app::AppContext, config::schema::BotConfig, error::Result,
    platform::telegram::TelegramPlatform,
};

pub async fn spawn_bots(ctx: &AppContext, engine: &AgentEngine) -> Result<()> {
    for (key, raw_cfg) in &ctx.cfg.bot {
        let bot_cfg = BotConfig::resolve(key, raw_cfg)?;
        let _handle = match bot_cfg.platform {
            crate::config::schema::BotPlatform::Telegram => {
                TelegramPlatform::spawn(&bot_cfg, ctx, engine).await?
            }
        };
    }
    Ok(())
}
