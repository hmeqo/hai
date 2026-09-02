use std::{collections::HashMap, sync::Arc};

use uuid::Uuid;

use crate::{
    agentcore::embedding::EmbeddingService,
    domain::{
        model::{Memory, MemoryKind},
        repo::Repos,
        vo::{ChatId, MemoryId},
    },
    error::{ErrorKind, Result},
    util::pgvector,
};

#[derive(Debug)]
pub struct RelatedMemory {
    pub id: MemoryId,
    pub content: String,
    pub account_id: Option<i64>,
    pub distance: f64,
    pub created_at: jiff::Timestamp,
}

#[derive(Debug)]
pub struct MemoryService {
    repos: Repos,
    embedding: Arc<dyn EmbeddingService>,
}

impl MemoryService {
    pub fn new(repos: Repos, embedding: Arc<dyn EmbeddingService>) -> Self {
        Self { repos, embedding }
    }

    pub async fn create(
        &self,
        kind: MemoryKind,
        chat_id: ChatId,
        content: String,
        account_id: Option<i64>,
        meta: Option<serde_json::Value>,
    ) -> Result<Memory> {
        if self
            .repos
            .memory
            .find_duplicate(kind.as_str(), chat_id.0, &content, account_id)
            .await?
            .is_some()
        {
            return Err(ErrorKind::AlreadyExists.msg(format!(
                "{} already exists with the same content",
                kind.as_str(),
            )));
        }

        let embedding = self.embedding.generate_embedding(&content).await?;

        let memory = self
            .repos
            .memory
            .create(
                Uuid::now_v7(),
                kind.as_str(),
                chat_id.0,
                account_id,
                &content,
                1,
                meta,
                jiff::Timestamp::now(),
                jiff::Timestamp::now(),
            )
            .await?;

        pgvector::upsert_embedding_vec(self.repos.pool(), "memory", memory.id, &embedding).await?;

        Ok(memory)
    }

    pub async fn update(
        &self,
        id: Uuid,
        content: Option<String>,
        importance: Option<i32>,
    ) -> Result<Memory> {
        let existing = self
            .repos
            .memory
            .find_by_id(id)
            .await?
            .ok_or_else(|| ErrorKind::NotFound.msg(format!("Memory not found: {id}")))?;

        self.repos
            .memory
            .update_fields(id, content.as_deref(), importance)
            .await?;

        if let Some(c) = &content {
            let emb = self.embedding.generate_embedding(c).await?;
            pgvector::upsert_embedding_vec(self.repos.pool(), "memory", id, &emb).await?;
        }

        Ok(existing)
    }

    pub async fn search_knowledge(
        &self,
        chat_id: ChatId,
        query: &str,
        limit: i64,
    ) -> Result<Vec<RelatedMemory>> {
        let embedding = self.embedding.generate_embedding(query).await?;
        self.search_related(chat_id, &embedding, limit).await
    }

    pub async fn search_related(
        &self,
        chat_id: ChatId,
        query: &[f32],
        limit: i64,
    ) -> Result<Vec<RelatedMemory>> {
        let rows = pgvector::search_embedding_vec(
            self.repos.pool(),
            "memory",
            query,
            Some(chat_id.0),
            None,
            limit,
        )
        .await?;
        if rows.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<Uuid> = rows.iter().map(|(id, _)| *id).collect();
        let memories: Vec<Memory> = self.repos.memory.by_ids(&ids).await?;

        let map: HashMap<Uuid, Memory> = memories.into_iter().map(|m| (m.id, m)).collect();

        Ok(rows
            .into_iter()
            .filter_map(|(id, dist)| {
                map.get(&id).map(|m| RelatedMemory {
                    id: MemoryId(m.id),
                    content: m.content.clone(),
                    account_id: m.account_id,
                    distance: dist,
                    created_at: m.created_at,
                })
            })
            .collect())
    }

    pub async fn delete(&self, id: MemoryId) -> Result<()> {
        self.repos.memory.delete_by_id(id.0).await?;
        if let Err(e) = pgvector::clear_embedding_vec(self.repos.pool(), "memory", id.0).await {
            tracing::warn!(memory_id = %id, "Failed to clear memory embedding: {e}");
        }
        Ok(())
    }
}
