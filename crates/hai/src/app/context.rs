use std::sync::Arc;

use arc_swap::ArcSwap;
use derive_more::Deref;

use crate::{
    agent::{multimodal::MultimodalService, runtime::AgentEventBus},
    config::{AppConfig, AppConfigManager, ProviderRegistry},
    domain::{db, repo::Repos, service::DbServices},
    error::Result,
};

#[derive(Clone, Deref)]
pub struct AppContext {
    inner: Arc<AppContextInner>,
}

pub struct AppContextInner {
    pub cfg_mgr: AppConfigManager,
    pub cfg: Arc<AppConfig>,
    pub provider: ProviderContext,
    pub db: DbContext,
    pub agent: AgentContext,
    pub event_bus: AgentEventBus,
}

impl AppContext {
    pub async fn new(cfg_mgr: AppConfigManager) -> Result<Self> {
        let cfg = cfg_mgr.load();

        let providers = ProviderRegistry::new(&cfg)?;
        let multimodal = MultimodalService::from_config(&cfg, &providers)?;

        let pool = db::init_db(&cfg.database).await?;
        let repos = Repos::new(pool.clone());
        let db_srv = DbServices::new(repos.clone(), multimodal.clone());

        let event_bus = AgentEventBus::new(repos);

        let provider = ProviderContext {
            provider: providers,
            multimodal,
        };
        let agent = AgentContext {
            current_model: ArcSwap::from_pointee(cfg.agent.model.clone()),
        };
        let db = DbContext { srv: db_srv };
        Ok(Self {
            inner: Arc::new(AppContextInner {
                cfg_mgr,
                cfg,
                provider,
                agent,
                db,
                event_bus,
            }),
        })
    }
}

pub struct DbContext {
    pub srv: DbServices,
}

#[derive(Deref)]
pub struct ProviderContext {
    #[deref]
    pub provider: ProviderRegistry,
    pub multimodal: MultimodalService,
}

pub struct AgentContext {
    pub current_model: ArcSwap<String>,
}

impl AgentContext {
    pub fn current_model(&self) -> Arc<String> {
        self.current_model.load().clone()
    }

    pub fn set_current_model(&self, model: String) {
        self.current_model.store(Arc::new(model));
    }
}
