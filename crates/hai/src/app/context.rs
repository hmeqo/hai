use std::sync::Arc;

use arc_swap::{ArcSwap, Guard};
use derive_more::Deref;
use sqlx::PgPool;
use teloxide::Bot;

use crate::{
    agent::{
        attachment::AttachmentService, context::ContextFactory, event::GroupTrigger,
        personality::PersonalityMgr,
    },
    agentcore::multimodal::{ModelService, MultimodalService},
    bot::telegram::{BotIdentity, TelegramService},
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
    pub bot: BotContext,
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

        let bot = Bot::new(&cfg.telegram.bot_token);
        let bot_account = db_srv.platform.ensure_bot_account().await?;
        let bot_identity = BotIdentity::new(bot_account, &bot).await?;
        let tg_srv = TelegramService::new(bot.clone());

        let file_cache = FileCache::new();
        let attachment = AttachmentService::new(
            file_cache,
            tg_srv.clone(),
            db_srv.clone(),
            multimodal.clone(),
        );

        let bot_state = BotContext {
            bot,
            identity: bot_identity.clone(),
            telegram: tg_srv,
        };
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
                agent,
                db,
                bot: bot_state,
            }),
        })
    }
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

#[derive(Deref)]
pub struct BotContext {
    #[deref]
    pub bot: Bot,
    pub identity: BotIdentity,
    pub telegram: TelegramService,
}

impl BotContext {
    pub fn account_id(&self) -> i64 {
        self.identity.account_id()
    }
}
