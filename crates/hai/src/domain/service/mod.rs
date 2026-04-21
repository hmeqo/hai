pub mod context;
pub mod identity;
pub mod memory;
pub mod message;
pub mod platform;
pub mod scratchpad;
pub mod topic;

pub use context::ContextService;
pub use identity::IdentityService;
pub use memory::MemoryService;
pub use message::MessageService;
pub use platform::PlatformService;
pub use scratchpad::ScratchpadService;
pub use topic::TopicService;

use sqlx::PgPool;
use std::sync::Arc;

use crate::agent::multimodal::EmbeddingService;
use crate::config::AppConfig;

pub struct Services {
    pub config: Arc<AppConfig>,
    pub platform: Arc<PlatformService>,
    pub identity: Arc<IdentityService>,
    pub topic: Arc<TopicService>,
    pub message: Arc<MessageService>,
    pub memory: Arc<MemoryService>,
    pub scratchpad: Arc<ScratchpadService>,
    pub context: Arc<ContextService>,
    pub embedding: Arc<EmbeddingService>,
}

impl Services {
    pub fn new(pool: PgPool, embedding: Arc<EmbeddingService>, config: Arc<AppConfig>) -> Self {
        let platform = Arc::new(PlatformService::new(pool.clone()));
        let identity = Arc::new(IdentityService::new(pool.clone()));
        let topic = Arc::new(TopicService::new(pool.clone(), embedding.clone()));
        let message = Arc::new(MessageService::new(pool.clone()));
        let memory = Arc::new(MemoryService::new(pool.clone(), embedding.clone()));
        let scratchpad = Arc::new(ScratchpadService::new(pool.clone()));
        let context = Arc::new(ContextService::new(
            Arc::clone(&config),
            platform.clone(),
            topic.clone(),
            message.clone(),
            memory.clone(),
            scratchpad.clone(),
            embedding.clone(),
        ));

        Self {
            config,
            platform,
            identity,
            topic,
            message,
            memory,
            scratchpad,
            context,
            embedding,
        }
    }
}
