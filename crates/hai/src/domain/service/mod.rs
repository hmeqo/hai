pub mod identity;
pub mod memory;
pub mod message;
pub mod perception;
pub mod platform;
pub mod scratchpad;
pub mod topic;

use std::sync::Arc;

use derive_more::Deref;
pub use identity::IdentityService;
pub use memory::MemoryService;
pub use message::{MessageService, NewAgentMessage, NewUserMessage};
pub use perception::PerceptionService;
pub use platform::PlatformService;
pub use scratchpad::ScratchpadService;
use sqlx::PgPool;
pub use topic::TopicService;

use crate::agentcore::multimodal::MultimodalService;

#[derive(Debug, Clone, Deref)]
pub struct DbServices(Arc<DbServicesInner>);

#[derive(Debug)]
pub struct DbServicesInner {
    pub platform: PlatformService,
    pub identity: IdentityService,
    pub topic: TopicService,
    pub message: MessageService,
    pub memory: MemoryService,
    pub scratchpad: ScratchpadService,
    pub multimodal: MultimodalService,
    pub perception: PerceptionService,
}

impl DbServices {
    pub fn new(pool: PgPool, multimodal: MultimodalService) -> Self {
        let platform = PlatformService::new(pool.clone());
        let identity = IdentityService::new(pool.clone());
        let message = MessageService::new(pool.clone());
        let scratchpad = ScratchpadService::new(pool.clone());
        let topic = TopicService::new(pool.clone(), multimodal.clone());
        let memory = MemoryService::new(pool.clone(), multimodal.clone());
        let perception = PerceptionService::new(pool.clone(), multimodal.clone());

        Self(Arc::new(DbServicesInner {
            platform,
            identity,
            topic,
            message,
            memory,
            scratchpad,
            multimodal,
            perception,
        }))
    }
}
