pub mod context;

use std::sync::Arc;

pub use context::AppContext;

use crate::{
    agent::{AgentGateway, handler::AgentCtx},
    bot::manager::spawn_bots,
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
        let agent_ctx = Arc::new(AgentCtx::new(ctx.clone()).await?);
        let mut gateway = AgentGateway::new(agent_ctx);

        spawn_bots(&ctx, &mut gateway).await?;

        let agent_handle = tokio::spawn(async move {
            let _ = gateway.run().await;
        });

        let _ = agent_handle.await;
        Ok(())
    }
}
