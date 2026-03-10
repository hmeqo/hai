use std::sync::Arc;

use anyhow::Result;
use pgvector::Vector;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    agent::multimodal::EmbeddingService,
    app::{
        domain::{
            entity::{Memory, MemoryType},
            model::MemoryInput,
        },
        repo::MemoryRepo,
    },
};

#[derive(Debug, Clone)]
pub struct RelatedMemory {
    pub content: String,
    pub account_id: Option<i64>,
    pub distance: f64,
}

/// 记忆管理服务
pub struct MemoryService {
    pool: PgPool,
    embedding: Arc<EmbeddingService>,
}

impl MemoryService {
    pub fn new(pool: PgPool, embedding: Arc<EmbeddingService>) -> Self {
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
                topic_id,
                message_id,
                content,
            } => {
                let mut memory = Memory::new(memory_type, content);
                memory.chat_id = Some(chat_id);
                memory.topic_id = topic_id;
                memory.source_message_id = message_id;
                MemoryRepo::create(&self.pool, memory).await
            }
            MemoryInput::CreateKnowledge { chat_id, content } => {
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
                )
                .await?
                .ok_or_else(|| anyhow::anyhow!("Memory not found: {}", id))
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
                    )
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("Failed to update rule"));
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

        // TODO: 这里的 account_ids 应该从当前上下文获取
        self.search_related_memories(&[], Some(chat_id), &vector, limit)
            .await
    }

    /// 综合检索相关记忆
    pub async fn search_related_memories(
        &self,
        account_ids: &[i64],
        chat_id: Option<i64>,
        query_vector: &Vector,
        limit: i64,
    ) -> Result<Vec<RelatedMemory>> {
        let results =
            MemoryRepo::search(&self.pool, account_ids, chat_id, query_vector, limit).await?;

        Ok(results
            .into_iter()
            .map(|r| RelatedMemory {
                content: r.memory.content,
                account_id: r.memory.account_id,
                distance: r.distance,
            })
            .collect())
    }

    /// 批量获取话题相关的记忆
    pub async fn get_memories_by_topics(&self, topic_ids: &[Uuid]) -> Result<Vec<Memory>> {
        MemoryRepo::list_by_topic_ids(&self.pool, topic_ids).await
    }

    /// 批量获取消息相关的记忆
    pub async fn get_memories_by_messages(&self, message_ids: &[i64]) -> Result<Vec<Memory>> {
        MemoryRepo::list_by_message_ids(&self.pool, message_ids).await
    }

    /// 获取群聊规则
    pub async fn get_chat_rule(&self, chat_id: i64) -> Result<Option<Memory>> {
        let rule_type: &'static str = MemoryType::Rule.into();
        MemoryRepo::find_by_type_and_chat(&self.pool, rule_type, chat_id).await
    }

    /// 删除知识
    pub async fn delete(&self, id: Uuid) -> Result<u64> {
        MemoryRepo::delete(&self.pool, id).await
    }
}
