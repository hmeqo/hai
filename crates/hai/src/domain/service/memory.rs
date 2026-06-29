use uuid::Uuid;

use crate::{
    agent::node::MultimodalService,
    domain::{
        model::{Memory, MemoryType},
        vo::{ChatId, MemoryId, MemoryInput},
    },
    error::{ErrorKind, Result},
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
    embedding: MultimodalService,
}

impl MemoryService {
    pub fn new(db: toasty::Db, embedding: MultimodalService) -> Self {
        Self { db, embedding }
    }

    async fn compute_embedding(
        &self,
        memory_type: MemoryType,
        content: &str,
    ) -> Result<Option<toasty::Json<Vec<f32>>>> {
        if memory_type.needs_embedding() {
            let e = self.embedding.generate_embedding(content).await?;
            Ok(Some(toasty::Json(e)))
        } else {
            Ok(None)
        }
    }

    pub async fn save_memory(&self, input: MemoryInput) -> Result<Memory> {
        let mut db = self.db.clone();
        let cid = |c: ChatId| c.0;

        match input {
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
                let embedding = self
                    .compute_embedding(MemoryType::UserFact, &content)
                    .await?;
                toasty::create!(Memory {
                    id: Uuid::now_v7(),
                    mem_type: "user_fact",
                    account_id: Some(account_id),
                    chat_id: Some(cid(chat_id)),
                    content,
                    embedding,
                    importance: 1,
                    last_accessed_at: jiff::Timestamp::now(),
                    created_at: jiff::Timestamp::now(),
                    updated_at: jiff::Timestamp::now(),
                })
                .exec(&mut db)
                .await
                .map_err(Into::into)
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
                create.exec(&mut db).await.map_err(Into::into)
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
                let embedding = self
                    .compute_embedding(MemoryType::Knowledge, &content)
                    .await?;
                toasty::create!(Memory {
                    id: Uuid::now_v7(),
                    mem_type: "knowledge",
                    chat_id: Some(cid(chat_id)),
                    content,
                    embedding,
                    importance: 1,
                    last_accessed_at: jiff::Timestamp::now(),
                    created_at: jiff::Timestamp::now(),
                    updated_at: jiff::Timestamp::now(),
                })
                .exec(&mut db)
                .await
                .map_err(Into::into)
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
                let embedding = if let Some(ref c) = content {
                    self.compute_embedding(existing.memory_type(), c)
                        .await
                        .unwrap_or(None)
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
                builder = builder.embedding(embedding);
                builder.exec(&mut db).await?;
                Ok(existing)
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
                toasty::create!(Memory {
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
                .await
                .map_err(Into::into)
            }
        }
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
        let memories: Vec<Memory> = Memory::filter(
            Memory::fields()
                .chat_id()
                .eq(Some(chat_id.0))
                .and(Memory::fields().mem_type().ne("rule")),
        )
        .exec(&mut self.db.clone())
        .await?;

        let mut scored: Vec<RelatedMemory> = memories
            .into_iter()
            .filter_map(|m| {
                let vec = m.embedding.as_ref()?;
                let dist = 1.0 - crate::util::vector::cosine_similarity(query, &vec.0)?;
                Some(RelatedMemory {
                    id: MemoryId(m.id),
                    content: m.content,
                    account_id: m.account_id,
                    distance: dist,
                    created_at: m.created_at,
                })
            })
            .collect();
        scored.sort_by(|a, b| {
            a.distance
                .partial_cmp(&b.distance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(limit as usize);
        Ok(scored)
    }

    pub async fn delete(&self, id: MemoryId) -> Result<()> {
        Memory::delete_by_id(&mut self.db.clone(), id.0).await?;
        Ok(())
    }
}
