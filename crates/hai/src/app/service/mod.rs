pub mod context;
pub mod identity;
pub mod memory;
pub mod message;
pub mod platform;
pub mod topic;

pub use context::ContextService;
pub use identity::IdentityService;
pub use memory::MemoryService;
pub use message::MessageService;
pub use platform::PlatformService;
pub use topic::TopicService;

use sqlx::PgPool;
use std::sync::Arc;

use crate::agent::multimodal::EmbeddingService;

pub struct ServiceContext {
    pub platform: Arc<PlatformService>,
    pub identity: Arc<IdentityService>,
    pub topic: Arc<TopicService>,
    pub message: Arc<MessageService>,
    pub memory: Arc<MemoryService>,
    pub context: Arc<ContextService>,
}

impl ServiceContext {
    pub fn new(pool: PgPool, embedding: Arc<EmbeddingService>) -> Self {
        let platform = Arc::new(PlatformService::new(pool.clone()));
        let identity = Arc::new(IdentityService::new(pool.clone()));
        let topic = Arc::new(TopicService::new(pool.clone(), embedding.clone()));
        let message = Arc::new(MessageService::new(pool.clone()));
        let memory = Arc::new(MemoryService::new(pool.clone(), embedding.clone()));
        let context = Arc::new(ContextService::new(
            embedding,
            platform.clone(),
            topic.clone(),
            message.clone(),
            memory.clone(),
        ));

        Self {
            platform,
            identity,
            topic,
            message,
            memory,
            context,
        }
    }
}
