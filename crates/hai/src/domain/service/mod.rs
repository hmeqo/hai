pub mod conversation;
pub mod conversation_record;
pub mod image;
pub mod knowledge;
pub mod memory;
pub mod message;
pub mod perception;
pub mod platform;
pub mod preference;
pub mod purge;
pub(crate) mod retrieval;
pub mod scheduled_task;
pub mod settings;
pub mod topic;

use std::sync::Arc;

pub use conversation::{ChatSummary, ConversationService, ConversationSummary};
pub use conversation_record::ConversationRecordService;
use derive_more::Deref;
pub use image::ImageStore;
pub use knowledge::{KnowledgeService, RelatedChunk};
pub use memory::{MemoryService, RecordOutcome};
pub use message::{MessageService, NewAgentMessage};
pub use perception::PerceptionService;
pub use platform::PlatformService;
pub use preference::PreferenceService;
pub use purge::PurgeService;
pub(crate) use retrieval::ScorerErrorPolicy;
pub use retrieval::{RecallItem, RecallQuery, RecallScope, recall};
pub use scheduled_task::ScheduledTaskService;
pub use settings::{SettingChange, SettingsChange, SettingsService};
pub use topic::TopicService;

use crate::{
    agentcore::api::{embedding::EmbeddingService, rerank::Scorer},
    domain::repo::Repos,
};

#[derive(Debug, Clone, Deref)]
pub struct Services(Arc<ServicesInner>);

#[derive(Debug)]
pub struct ServicesInner {
    pub platform: PlatformService,
    pub topic: TopicService,
    pub message: MessageService,
    pub memory: MemoryService,
    pub knowledge: KnowledgeService,
    pub conversation: ConversationRecordService,
    pub scheduled_task: ScheduledTaskService,
    pub embedding: Arc<dyn EmbeddingService>,
    /// Absent when `[model-roles.rerank]` is not configured; retrieval then keeps the fusion order.
    pub scorer: Option<Scorer>,
    pub perception: PerceptionService,
    pub preference: PreferenceService,
    pub purge: PurgeService,
    pub images: ImageStore,
}

impl Services {
    pub fn new(
        repos: Repos,
        embedding: Arc<dyn EmbeddingService>,
        scorer: Option<Scorer>,
        images: ImageStore,
    ) -> Self {
        let platform = PlatformService::new(repos.clone());
        let message = MessageService::new(repos.clone());
        let conversation = ConversationRecordService::new(repos.clone());
        let scheduled_task = ScheduledTaskService::new(repos.clone());
        let topic = TopicService::new(repos.clone(), Arc::clone(&embedding));
        let memory = MemoryService::new(repos.clone(), Arc::clone(&embedding));
        let perception = PerceptionService::new(repos.clone(), Arc::clone(&embedding));
        let knowledge =
            KnowledgeService::new(repos.clone(), Arc::clone(&embedding), scorer.clone());
        let preference = PreferenceService::new(repos.clone());
        let purge = PurgeService::new(repos.clone());

        Self(Arc::new(ServicesInner {
            platform,
            topic,
            message,
            memory,
            knowledge,
            conversation,
            scheduled_task,
            embedding,
            perception,
            preference,
            purge,
            images,
            scorer,
        }))
    }
}
