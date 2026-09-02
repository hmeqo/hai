pub mod conversation_record;
pub mod identity;
pub mod knowledge;
pub mod memory;
pub mod message;
pub mod perception;
pub mod platform;
pub mod scheduled_task;
pub mod scratchpad;
pub mod topic;

use std::sync::Arc;

pub use conversation_record::ConversationRecordService;
use derive_more::Deref;
pub use identity::IdentityService;
pub use knowledge::{KnowledgeService, RelatedChunk};
pub use memory::MemoryService;
pub use message::{MessageService, NewAgentMessage, NewUserMessage};
pub use perception::PerceptionService;
pub use platform::PlatformService;
pub use scheduled_task::ScheduledTaskService;
pub use scratchpad::ScratchpadService;
pub use topic::TopicService;

use crate::{
    agent::multimodal::MultimodalService, agentcore::embedding::EmbeddingService,
    domain::repo::Repos,
};

#[derive(Debug, Clone, Deref)]
pub struct DbServices(Arc<DbServicesInner>);

#[derive(Debug)]
pub struct DbServicesInner {
    pub platform: PlatformService,
    pub identity: IdentityService,
    pub topic: TopicService,
    pub message: MessageService,
    pub memory: MemoryService,
    pub knowledge: KnowledgeService,
    pub conversation: ConversationRecordService,
    pub scratchpad: ScratchpadService,
    pub scheduled_task: ScheduledTaskService,
    pub multimodal: MultimodalService,
    pub perception: PerceptionService,
}

impl DbServices {
    pub fn new(repos: Repos, multimodal: MultimodalService) -> Self {
        let platform = PlatformService::new(repos.clone());
        let identity = IdentityService::new(repos.clone());
        let message = MessageService::new(repos.clone());
        let conversation = ConversationRecordService::new(repos.clone());
        let scratchpad = ScratchpadService::new(repos.clone());
        let scheduled_task = ScheduledTaskService::new(repos.clone());
        let embedding: Arc<dyn EmbeddingService> = Arc::new(multimodal.clone());
        let topic = TopicService::new(repos.clone(), Arc::clone(&embedding));
        let memory = MemoryService::new(repos.clone(), Arc::clone(&embedding));
        let perception = PerceptionService::new(repos.clone(), Arc::clone(&embedding));
        let knowledge = KnowledgeService::new(repos.clone(), Arc::clone(&embedding));

        Self(Arc::new(DbServicesInner {
            platform,
            identity,
            topic,
            message,
            memory,
            knowledge,
            conversation,
            scratchpad,
            scheduled_task,
            multimodal,
            perception,
        }))
    }
}
