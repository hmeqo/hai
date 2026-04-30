use std::sync::Arc;

use arc_swap::{ArcSwap, Guard};
use derive_more::Deref;
use sqlx::PgPool;
use teloxide::Bot;

use crate::{
    agent::{
        attachment::AttachmentService,
        context::ContextFactory,
        event::GroupTrigger,
        node::{ModelService, MultimodalService},
        personality::PersonalityMgr,
    },
    bot::telegram::TelegramService,
    config::{AppConfig, AppConfigManager, ProviderManager},
    domain::{db, service::DbServices},
    error::Result,
    infra::cache::FileCache,
};

#[derive(Clone, Deref)]
pub struct AppContext {
    inner: Arc<AppContextInner>,
}

pub struct AppContextInner {
    pub cfg_mgr: AppConfigManager,
    pub cfg: Arc<AppConfig>,
    pub provider: ProviderContext,
    pub file_service: TelegramService,
    pub db: DbContext,
    pub agent: AgentContext,
}

impl AppContext {
    pub async fn new(cfg_mgr: AppConfigManager) -> Result<Self> {
        let cfg = cfg_mgr.load();

        let providers = ProviderManager::new(&cfg)?;
        let multimodal = MultimodalService::from_config(&cfg, &providers);
        let personality = PersonalityMgr::new(Arc::clone(&cfg));
        let group_trigger = Arc::new(
            GroupTrigger::new()
                .with_min_heat(personality.min_heat(&cfg.agent.trigger))
                .with_conversation_window_secs(personality.conversation_window_secs()),
        );

        let pool = db::init_pool(&cfg.database).await?;
        let db_srv = DbServices::new(pool.clone(), multimodal.clone());

        let context_fty = ContextFactory::new(Arc::clone(&cfg), db_srv.clone());

        // 创建文件下载服务（取第一个 Telegram bot 的 token）
        let file_service = create_telegram_service(&cfg);

        let file_cache = FileCache::new();
        let attachment = AttachmentService::new(
            file_cache,
            file_service.clone(),
            db_srv.clone(),
            multimodal.clone(),
        );

        let provider = ProviderContext {
            provider: providers,
            multimodal,
            model: ModelService::new(cfg.model.clone()),
        };
        let agent = AgentContext {
            personality,
            context_fty,
            group_trigger,
            attachment,
            current_model: ArcSwap::from_pointee(cfg.agent.model.clone()),
        };
        let db = DbContext { pool, srv: db_srv };
        Ok(Self {
            inner: Arc::new(AppContextInner {
                cfg_mgr,
                cfg,
                provider,
                file_service,
                agent,
                db,
            }),
        })
    }
}

/// 从第一个配置的 Telegram bot 创建文件下载服务
fn create_telegram_service(cfg: &AppConfig) -> TelegramService {
    for (_, raw) in &cfg.bot {
        if raw.bot_type.as_deref().unwrap_or("telegram") == "telegram"
            && raw.bot_token.as_deref().is_some_and(|t| !t.is_empty())
        {
            return TelegramService::new(Bot::new(raw.bot_token.as_deref().unwrap_or("")));
        }
    }
    tracing::warn!("No telegram bot configured — file download unavailable");
    TelegramService::new(Bot::new(""))
}

pub struct DbContext {
    pub pool: PgPool,
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
    pub personality: PersonalityMgr,
    pub context_fty: ContextFactory,
    pub group_trigger: Arc<GroupTrigger>,
    pub attachment: AttachmentService,
    pub current_model: ArcSwap<String>,
}

impl AgentContext {
    pub fn current_model(&self) -> Guard<Arc<String>> {
        self.current_model.load()
    }

    pub fn set_current_model(&self, model: String) {
        self.current_model.store(Arc::new(model));
    }
}
