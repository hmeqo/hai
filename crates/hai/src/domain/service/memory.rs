use std::{collections::HashMap, sync::Arc};

use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    agentcore::embedding::EmbeddingService,
    domain::{
        model::{Memory, MemoryKind},
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
    db: toasty::Db,
    embedding: Arc<dyn EmbeddingService>,
    pool: PgPool,
}

impl MemoryService {
    pub fn new(db: toasty::Db, embedding: Arc<dyn EmbeddingService>, pool: PgPool) -> Self {
        Self {
            db,
            embedding,
            pool,
        }
    }

    pub async fn create(
        &self,
        kind: MemoryKind,
        chat_id: ChatId,
        content: String,
        account_id: Option<i64>,
        meta: Option<serde_json::Value>,
    ) -> Result<Memory> {
        let mut db = self.db.clone();

        let mut expr = Memory::fields()
            .kind()
            .eq(kind.as_str())
            .and(Memory::fields().chat_id().eq(Some(chat_id.0)))
            .and(Memory::fields().content().eq(&content));
        if let Some(aid) = account_id {
            expr = expr.and(Memory::fields().account_id().eq(Some(aid)));
        }

        if Memory::filter(expr).first().exec(&mut db).await?.is_some() {
            return Err(ErrorKind::AlreadyExists.msg(format!(
                "{} already exists with the same content",
                kind.as_str(),
            )));
        }

        let embedding = self.embedding.generate_embedding(&content).await?;

        let memory = toasty::create!(Memory {
            id: Uuid::now_v7(),
            kind: kind.as_str(),
            chat_id: Some(chat_id.0),
            account_id,
            content,
            importance: 1,
            meta: meta.map(toasty::Json),
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        })
        .exec(&mut db)
        .await?;

        pgvector::upsert_embedding_vec(&self.pool, "memory", memory.id, &embedding).await?;

        Ok(memory)
    }

    pub async fn update(
        &self,
        id: Uuid,
        content: Option<String>,
        importance: Option<i32>,
    ) -> Result<Memory> {
        let mut db = self.db.clone();

        let existing = Memory::get_by_id(&mut db, &id)
            .await
            .map_err(|_| ErrorKind::NotFound.msg(format!("Memory not found: {id}")))?;

        let mut builder = Memory::filter_by_id(id).update();
        if let Some(ref c) = content {
            builder = builder.content(c);
        }
        if let Some(imp) = importance {
            builder = builder.importance(imp);
        }
        builder.exec(&mut db).await?;

        if let Some(ref c) = content {
            let emb = self.embedding.generate_embedding(c).await?;
            pgvector::upsert_embedding_vec(&self.pool, "memory", id, &emb).await?;
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
        let rows =
            pgvector::search_embedding_vec(&self.pool, "memory", query, chat_id.0, None, limit)
                .await?;
        if rows.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<Uuid> = rows.iter().map(|(id, _)| *id).collect();
        let memories: Vec<Memory> = Memory::filter(Memory::fields().id().in_list(ids))
            .exec(&mut self.db.clone())
            .await?;

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
        Memory::delete_by_id(&mut self.db.clone(), id.0).await?;
        if let Err(e) = pgvector::clear_embedding_vec(&self.pool, "memory", id.0).await {
            tracing::warn!(memory_id = %id, "Failed to clear memory embedding: {e}");
        }
        Ok(())
    }
}
