pub mod account;
pub mod attachment;
pub mod chat;
pub mod conversation_preference;
pub mod conversation_record;
pub mod event;
pub mod ingest;
pub mod knowledge_chunk;
pub mod knowledge_document;
pub(crate) mod lexical;
pub mod memory;
pub mod message;
pub mod perception;
pub mod platform_channel;
pub mod purge;
pub mod scheduled_task;
pub mod topic;
pub mod vector;
pub(crate) mod window;

pub use account::AccountRepo;
pub use attachment::AttachmentRepo;
pub use chat::ChatRepo;
pub use conversation_preference::ConversationPreferenceRepo;
pub use conversation_record::ConversationRecordRepo;
pub use event::{EventQuery, EventRepo};
pub use ingest::{ChangeKind, IngestRepo, PlatformChange};
pub use knowledge_chunk::KnowledgeChunkRepo;
pub use knowledge_document::KnowledgeDocumentRepo;
pub use memory::MemoryRepo;
pub use message::MessageRepo;
pub use perception::PerceptionRepo;
pub use platform_channel::PlatformChannelRepo;
pub use purge::{PurgeRepo, PurgeWiped};
pub use scheduled_task::ScheduledTaskRepo;
use sqlx::PgPool;
pub use topic::TopicRepo;

#[derive(Debug, Clone)]
pub struct Repos {
    pub account: AccountRepo,
    pub attachment: AttachmentRepo,
    pub chat: ChatRepo,
    pub conversation_preference: ConversationPreferenceRepo,
    pub conversation_record: ConversationRecordRepo,
    pub platform_channel: PlatformChannelRepo,
    pub event: EventRepo,
    pub ingest: IngestRepo,
    pub knowledge_chunk: KnowledgeChunkRepo,
    pub knowledge_document: KnowledgeDocumentRepo,
    pub memory: MemoryRepo,
    pub message: MessageRepo,
    pub perception: PerceptionRepo,
    pub purge: PurgeRepo,
    pub scheduled_task: ScheduledTaskRepo,
    pub topic: TopicRepo,
}

impl Repos {
    pub fn new(pool: PgPool) -> Self {
        Self {
            account: AccountRepo::new(pool.clone()),
            attachment: AttachmentRepo::new(pool.clone()),
            chat: ChatRepo::new(pool.clone()),
            conversation_preference: ConversationPreferenceRepo::new(pool.clone()),
            conversation_record: ConversationRecordRepo::new(pool.clone()),
            platform_channel: PlatformChannelRepo::new(pool.clone()),
            event: EventRepo::new(pool.clone()),
            ingest: IngestRepo::new(pool.clone()),
            knowledge_chunk: KnowledgeChunkRepo::new(pool.clone()),
            knowledge_document: KnowledgeDocumentRepo::new(pool.clone()),
            memory: MemoryRepo::new(pool.clone()),
            message: MessageRepo::new(pool.clone()),
            perception: PerceptionRepo::new(pool.clone()),
            purge: PurgeRepo::new(pool.clone()),
            scheduled_task: ScheduledTaskRepo::new(pool.clone()),
            topic: TopicRepo::new(pool.clone()),
        }
    }
}
