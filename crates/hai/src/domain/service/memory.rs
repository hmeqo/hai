use pgvector::Vector;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    agentcore::multimodal::MultimodalService,
    domain::{
        entity::{Memory, MemoryType},
        repo::MemoryRepo,
        vo::MemoryInput,
    },
    error::{ErrorKind, OptionAppExt, Result},
};

#[derive(Debug, Clone)]
pub struct RelatedMemory {
    pub id: Uuid,
    pub content: String,
    pub account_id: Option<i64>,
    pub distance: f64,
    pub created_at: jiff::Timestamp,
}

/// 记忆管理服务
#[derive(Debug)]
pub struct MemoryService {
    pool: PgPool,
    embedding: MultimodalService,
}

impl MemoryService {
    pub fn new(pool: PgPool, embedding: MultimodalService) -> Self {
        Self { pool, embedding }
    }

    /// 根据 content 字符串计算 embedding（当 memory_type 需要时）
    async fn compute_embedding_if_needed(
        &self,
        memory_type: MemoryType,
        content: &str,
    ) -> Result<Option<Vector>> {
        if memory_type.needs_embedding() {
            let e = self.embedding.generate_embedding(content).await?;
            Ok(Some(Vector::from(e)))
        } else {
            Ok(None)
        }
    }

    /// 统一保存记忆接口 (新增/修改/覆盖)
    pub async fn save_memory(&self, input: MemoryInput) -> Result<Memory> {
        let memory_type = input.memory_type();

        match input {
            // --- Create Variants ---
            MemoryInput::CreateUserFact {
                account_id,
                chat_id,
                content,
            } => {
                if MemoryRepo::find_user_fact(&self.pool, account_id, chat_id, &content)
                    .await?
                    .is_some()
                {
                    return Err(ErrorKind::AlreadyExists.with_msg(
                        "UserFact already exists for this account and chat with the same content",
                    ));
                }
                let embedding = self
                    .compute_embedding_if_needed(memory_type, &content)
                    .await?;
                let mut memory = Memory::new(memory_type, content);
                memory.account_id = Some(account_id);
                memory.chat_id = Some(chat_id);
                memory.embedding = embedding;
                MemoryRepo::create(&self.pool, memory).await
            }
            MemoryInput::CreateAgentNote {
                chat_id,
                references,
                content,
            } => {
                if MemoryRepo::find_agent_note(&self.pool, chat_id, &content)
                    .await?
                    .is_some()
                {
                    return Err(ErrorKind::AlreadyExists
                        .with_msg("AgentNote already exists for this chat with the same content"));
                }
                let mut memory = Memory::new(memory_type, content);
                memory.chat_id = Some(chat_id);
                memory.references = references;
                MemoryRepo::create(&self.pool, memory).await
            }
            MemoryInput::CreateKnowledge { chat_id, content } => {
                if MemoryRepo::find_knowledge(&self.pool, chat_id, &content)
                    .await?
                    .is_some()
                {
                    return Err(ErrorKind::AlreadyExists
                        .with_msg("Knowledge already exists for this chat with the same content"));
                }
                let embedding = self
                    .compute_embedding_if_needed(memory_type, &content)
                    .await?;
                let mut memory = Memory::new(memory_type, content);
                memory.chat_id = Some(chat_id);
                memory.embedding = embedding;
                MemoryRepo::create(&self.pool, memory).await
            }

            // --- Update Variants ---
            MemoryInput::UpdateUserFact {
                id,
                content,
                importance,
            }
            | MemoryInput::UpdateAgentNote {
                id,
                content,
                importance,
            }
            | MemoryInput::UpdateKnowledge {
                id,
                content,
                importance,
            } => {
                let embedding = if let Some(new_content) = &content {
                    self.compute_embedding_if_needed(memory_type, new_content)
                        .await?
                } else {
                    None
                };

                MemoryRepo::update(
                    &self.pool,
                    id,
                    content.as_deref(),
                    importance,
                    None,
                    embedding,
                    None,
                )
                .await?
                .ok_or_err_msg(ErrorKind::NotFound, format!("Memory not found: {}", id))
            }

            // --- Upsert Variants ---
            MemoryInput::UpsertChatRule { chat_id, content } => {
                if let Some(existing) =
                    MemoryRepo::find_by_type_and_chat(&self.pool, memory_type.into(), chat_id)
                        .await?
                {
                    return MemoryRepo::update(
                        &self.pool,
                        existing.id,
                        Some(&content),
                        None,
                        None,
                        None,
                        None,
                    )
                    .await?
                    .ok_or_err_msg(ErrorKind::Internal, "Failed to update rule");
                }

                let mut memory = Memory::new(memory_type, content);
                memory.chat_id = Some(chat_id);
                memory.importance = 10;

                MemoryRepo::create(&self.pool, memory).await
            }
        }
    }

    /// 语义搜索知识
    pub async fn search_knowledge(
        &self,
        chat_id: i64,
        query: &str,
        limit: i64,
    ) -> Result<Vec<RelatedMemory>> {
        let embedding = self.embedding.generate_embedding(query).await?;
        let vector = pgvector::Vector::from(embedding);

        self.search_related_memories(chat_id, &vector, limit).await
    }

    /// 综合检索相关记忆
    pub async fn search_related_memories(
        &self,
        chat_id: i64,
        query_vector: &Vector,
        limit: i64,
    ) -> Result<Vec<RelatedMemory>> {
        let results = MemoryRepo::search(&self.pool, chat_id, query_vector, limit).await?;

        Ok(results
            .into_iter()
            .map(|r| {
                let created_at = r.memory.created_at();
                RelatedMemory {
                    id: r.memory.id,
                    content: r.memory.content,
                    account_id: r.memory.account_id,
                    distance: r.distance,
                    created_at,
                }
            })
            .collect())
    }

    /// 删除记忆
    pub async fn delete(&self, id: Uuid) -> Result<u64> {
        MemoryRepo::delete(&self.pool, id).await
    }
}
