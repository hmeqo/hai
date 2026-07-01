use std::sync::Arc;

use arc_swap::ArcSwap;
use derive_more::Deref;

use crate::{
    agent::node::{ModelService, MultimodalService},
    config::{AppConfig, AppConfigManager, ProviderManager},
    domain::{db, service::DbServices},
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
}

impl AppContext {
    pub async fn new(cfg_mgr: AppConfigManager) -> Result<Self> {
        let cfg = cfg_mgr.load();

        let providers = ProviderManager::new(&cfg)?;
        let multimodal = MultimodalService::from_config(&cfg, &providers);

        let (db_handle, pool) = db::init_db(&cfg.database).await?;
        let db_srv = DbServices::new(db_handle.clone(), pool, multimodal.clone());

        let provider = ProviderContext {
            provider: providers,
            multimodal,
            model: ModelService::new(cfg.model.clone()),
        };
        let agent = AgentContext {
            current_model: ArcSwap::from_pointee(cfg.agent.model.clone()),
        };
        let db = DbContext {
            pool: db_handle,
            srv: db_srv,
        };
        Ok(Self {
            inner: Arc::new(AppContextInner {
                cfg_mgr,
                cfg,
                provider,
                agent,
                db,
            }),
        })
    }
}

pub struct DbContext {
    pub pool: toasty::Db,
    pub srv: DbServices,
}

#[derive(Deref)]
pub struct ProviderContext {
    #[deref]
    pub provider: ProviderManager,
    pub multimodal: MultimodalService,
    pub model: ModelService,
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
