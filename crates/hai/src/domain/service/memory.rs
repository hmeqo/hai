use std::{collections::HashMap, sync::Arc};

use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    agentcore::embedding::EmbeddingService,
    domain::{
        model::{Memory, MemoryType},
        vo::{ChatId, MemoryId, MemoryInput},
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

    async fn compute_embedding(
        &self,
        memory_type: MemoryType,
        content: &str,
    ) -> Result<Option<Vec<f32>>> {
        if memory_type.needs_embedding() {
            let e = self.embedding.generate_embedding(content).await?;
            Ok(Some(e))
        } else {
            Ok(None)
        }
    }

    pub async fn save_memory(&self, input: MemoryInput) -> Result<Memory> {
        let mut db = self.db.clone();
        let cid = |c: ChatId| c.0;

        let (memory, emb_vec) = match input {
            MemoryInput::CreateUserFact {
                account_id,
                chat_id,
                content,
            } => {
                if Memory::filter(
                    Memory::fields()
                        .mem_type()
                        .eq("user_fact")
                        .and(Memory::fields().chat_id().eq(Some(cid(chat_id))))
                        .and(Memory::fields().account_id().eq(Some(account_id)))
                        .and(Memory::fields().content().eq(&content)),
                )
                .first()
                .exec(&mut db)
                .await?
                .is_some()
                {
                    return Err(ErrorKind::AlreadyExists.msg(
                        "UserFact already exists for this account and chat with the same content",
                    ));
                }
                let emb_vec = self
                    .compute_embedding(MemoryType::UserFact, &content)
                    .await?;
                let memory = toasty::create!(Memory {
                    id: Uuid::now_v7(),
                    mem_type: "user_fact",
                    account_id: Some(account_id),
                    chat_id: Some(cid(chat_id)),
                    content,
                    importance: 1,
                    last_accessed_at: jiff::Timestamp::now(),
                    created_at: jiff::Timestamp::now(),
                    updated_at: jiff::Timestamp::now(),
                })
                .exec(&mut db)
                .await?;
                (memory, emb_vec)
            }

            MemoryInput::CreateAgentNote {
                chat_id,
                references,
                content,
            } => {
                if Memory::filter(
                    Memory::fields()
                        .mem_type()
                        .eq("agent_note")
                        .and(Memory::fields().chat_id().eq(Some(cid(chat_id))))
                        .and(Memory::fields().content().eq(&content)),
                )
                .first()
                .exec(&mut db)
                .await?
                .is_some()
                {
                    return Err(ErrorKind::AlreadyExists
                        .msg("AgentNote already exists for this chat with the same content"));
                }
                let mut create = Memory::create()
                    .mem_type("agent_note")
                    .chat_id(Some(cid(chat_id)))
                    .content(&content)
                    .importance(1);
                if let Some(refs) = references {
                    create = create.references(toasty::Json(refs));
                }
                let memory = create.exec(&mut db).await?;
                (memory, None)
            }

            MemoryInput::CreateKnowledge { chat_id, content } => {
                if Memory::filter(
                    Memory::fields()
                        .mem_type()
                        .eq("knowledge")
                        .and(Memory::fields().chat_id().eq(Some(cid(chat_id))))
                        .and(Memory::fields().content().eq(&content)),
                )
                .first()
                .exec(&mut db)
                .await?
                .is_some()
                {
                    return Err(ErrorKind::AlreadyExists
                        .msg("Knowledge already exists for this chat with the same content"));
                }
                let emb_vec = self
                    .compute_embedding(MemoryType::Knowledge, &content)
                    .await?;
                let memory = toasty::create!(Memory {
                    id: Uuid::now_v7(),
                    mem_type: "knowledge",
                    chat_id: Some(cid(chat_id)),
                    content,
                    importance: 1,
                    last_accessed_at: jiff::Timestamp::now(),
                    created_at: jiff::Timestamp::now(),
                    updated_at: jiff::Timestamp::now(),
                })
                .exec(&mut db)
                .await?;
                (memory, emb_vec)
            }

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
                let existing = Memory::get_by_id(&mut db, &id)
                    .await
                    .map_err(|_| ErrorKind::NotFound.msg(format!("Memory not found: {id}")))?;
                let emb_vec = if let Some(ref c) = content {
                    match self.compute_embedding(existing.memory_type(), c).await {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::warn!(memory_id = %id, "Embedding update failed: {e}");
                            None
                        }
                    }
                } else {
                    None
                };
                let mut builder = Memory::filter_by_id(id).update();
                if let Some(ref c) = content {
                    builder = builder.content(c);
                }
                if let Some(imp) = importance {
                    builder = builder.importance(imp);
                }
                builder.exec(&mut db).await?;

                if let Some(ref emb) = emb_vec {
                    let _ = pgvector::upsert_embedding_vec(&self.pool, "memory", id, emb).await;
                }
                return Ok(existing);
            }

            MemoryInput::UpsertChatRule { chat_id, content } => {
                if let Some(mut existing) = Memory::filter(
                    Memory::fields()
                        .mem_type()
                        .eq("rule")
                        .and(Memory::fields().chat_id().eq(Some(cid(chat_id)))),
                )
                .first()
                .exec(&mut db)
                .await?
                {
                    toasty::update!(existing { content }).exec(&mut db).await?;
                    return Ok(existing);
                }
                let memory = toasty::create!(Memory {
                    id: Uuid::now_v7(),
                    mem_type: "rule",
                    chat_id: Some(cid(chat_id)),
                    content,
                    importance: 10,
                    last_accessed_at: jiff::Timestamp::now(),
                    created_at: jiff::Timestamp::now(),
                    updated_at: jiff::Timestamp::now(),
                })
                .exec(&mut db)
                .await?;
                (memory, None)
            }
        };

        if let Some(ref emb) = emb_vec {
            let _ = pgvector::upsert_embedding_vec(&self.pool, "memory", memory.id, emb).await;
        }
        Ok(memory)
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
        let filter = format!("chat_id = {} AND \"type\" != 'rule'", chat_id.0);
        let rows =
            pgvector::search_embedding_vec(&self.pool, "memory", query, &filter, limit).await?;
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
        let _ = pgvector::clear_embedding_vec(&self.pool, "memory", id.0).await;
        Ok(())
    }
}
