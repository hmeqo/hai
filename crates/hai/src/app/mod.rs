pub mod context;

use std::sync::Arc;

pub use context::AppContext;

use crate::{
    agent::{AgentGateway, runtime::AgentCtx},
    config::AppConfigManager,
    error::Result,
    platform::manager::spawn_bots,
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
        let agent_ctx = Arc::new(AgentCtx::new(ctx.clone()).await?);
        let mut gateway = AgentGateway::new(agent_ctx);

        spawn_bots(&ctx, &mut gateway).await?;

        let agent_handle = tokio::spawn(async move {
            if let Err(err) = gateway.run().await {
                tracing::error!("Agent gateway failed: {err}");
            }
        });

        agent_handle.await?;

        Ok(())
    }
}
