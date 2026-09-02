pub mod account;
pub mod chat;
pub mod conversation_record;
pub mod event;
pub mod identity;
pub mod knowledge_chunk;
pub mod knowledge_document;
pub mod memory;
pub mod message;
pub mod perception;
pub mod scheduled_task;
pub mod scratchpad;
pub mod topic;

pub use account::AccountRepo;
pub use chat::ChatRepo;
pub use conversation_record::ConversationRecordRepo;
pub use event::EventRepo;
pub use identity::IdentityRepo;
pub use knowledge_chunk::KnowledgeChunkRepo;
pub use knowledge_document::KnowledgeDocumentRepo;
pub use memory::MemoryRepo;
pub use message::MessageRepo;
pub use perception::PerceptionRepo;
pub use scheduled_task::ScheduledTaskRepo;
pub use scratchpad::ScratchpadRepo;
use sqlx::PgPool;
pub use topic::TopicRepo;

/// 数据访问聚合：service 只经此层访问 DB——SQL 全封装在 repo 内。
#[derive(Debug, Clone)]
pub struct Repos {
    pool: PgPool,
    pub account: AccountRepo,
    pub chat: ChatRepo,
    pub conversation_record: ConversationRecordRepo,
    pub event: EventRepo,
    pub identity: IdentityRepo,
    pub knowledge_chunk: KnowledgeChunkRepo,
    pub knowledge_document: KnowledgeDocumentRepo,
    pub memory: MemoryRepo,
    pub message: MessageRepo,
    pub perception: PerceptionRepo,
    pub scratchpad: ScratchpadRepo,
    pub scheduled_task: ScheduledTaskRepo,
    pub topic: TopicRepo,
}

impl Repos {
    /// 连接池访问（pgvector 等 sqlx util 用——service 不直接持 sqlx 类型）
    pub(crate) fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub fn new(pool: PgPool) -> Self {
        Self {
            pool: pool.clone(),
            account: AccountRepo::new(pool.clone()),
            chat: ChatRepo::new(pool.clone()),
            conversation_record: ConversationRecordRepo::new(pool.clone()),
            event: EventRepo::new(pool.clone()),
            identity: IdentityRepo::new(pool.clone()),
            knowledge_chunk: KnowledgeChunkRepo::new(pool.clone()),
            knowledge_document: KnowledgeDocumentRepo::new(pool.clone()),
            memory: MemoryRepo::new(pool.clone()),
            message: MessageRepo::new(pool.clone()),
            perception: PerceptionRepo::new(pool.clone()),
            scratchpad: ScratchpadRepo::new(pool.clone()),
            scheduled_task: ScheduledTaskRepo::new(pool.clone()),
            topic: TopicRepo::new(pool.clone()),
        }
    }
}
