pub mod context;

pub use context::AppContext;
use tracing_subscriber::EnvFilter;

use crate::{
    agent::runtime::AgentEngine, config::AppConfigManager, error::Result,
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

        let filter = EnvFilter::new(format!("{},rmcp=warn", cfg.logging.level()));
        tracing_subscriber::FmtSubscriber::builder()
            .with_env_filter(filter)
            .init();

        App::new(config_mgr).run().await
    }

    pub async fn run(self) -> Result<()> {
        let ctx = AppContext::new(self.config_mgr).await?;
        let engine = AgentEngine::new(ctx.clone()).await?;

        spawn_bots(&ctx, &engine).await?;

        // 主线程保持运行
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Shutting down...");
        Ok(())
    }
}
