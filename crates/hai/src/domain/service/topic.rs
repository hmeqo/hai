use std::{collections::HashMap, sync::Arc};

use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    agentcore::embedding::EmbeddingService,
    domain::{
        model::{Message, MessageStatus, Topic},
        vo::{ChatId, MessageId, TopicSearchResult},
    },
    error::Result,
    util::pgvector,
};

#[derive(Debug)]
pub struct TopicService {
    db: toasty::Db,
    embedding: Arc<dyn EmbeddingService>,
    pool: PgPool,
}

impl TopicService {
    pub fn new(db: toasty::Db, embedding: Arc<dyn EmbeddingService>, pool: PgPool) -> Self {
        Self {
            db,
            embedding,
            pool,
        }
    }

    pub async fn create_topic(
        &self,
        chat_id: ChatId,
        title: &str,
        summary: &str,
        message_ids: &[MessageId],
        meta: Option<serde_json::Value>,
    ) -> Result<Topic> {
        let mut db = self.db.clone();
        let mut tx = db.transaction().await?;
        let now = jiff::Timestamp::now();

        let topic = toasty::create!(Topic {
            id: uuid::Uuid::now_v7(),
            chat_id: chat_id.0,
            title,
            summary,
            status: "active",
            started_at: now,
            last_active_at: now,
            message_count: 0,
            meta: meta.map(toasty::Json),
            created_at: now,
            updated_at: now,
        })
        .exec(&mut tx)
        .await?;

        if !message_ids.is_empty() {
            Message::filter(
                Message::fields()
                    .id()
                    .in_list(MessageId::raw_ids(message_ids)),
            )
            .update()
            .topic_id(topic.id)
            .interaction_status(MessageStatus::Seen.as_str())
            .exec(&mut tx)
            .await?;
        }

        tx.commit().await?;
        Ok(topic)
    }

    pub async fn assign_topic(&self, message_ids: &[MessageId], topic_id: Uuid) -> Result<()> {
        let mut db = self.db.clone();
        let mut tx = db.transaction().await?;

        Message::filter(
            Message::fields()
                .id()
                .in_list(MessageId::raw_ids(message_ids)),
        )
        .update()
        .topic_id(topic_id)
        .interaction_status(MessageStatus::Seen.as_str())
        .exec(&mut tx)
        .await?;

        sync_topic_times(&mut tx, topic_id).await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn append_summary(&self, topic_id: Uuid, new_summary: &str) -> Result<Topic> {
        let mut db = self.db.clone();
        let topic = Topic::get_by_id(&mut db, &topic_id).await?;
        topic.ensure_not_closed()?;

        let formatted = format!("\n---\n{new_summary}");
        let combined = match topic.summary {
            Some(ref s) => format!("{s}{formatted}"),
            None => formatted,
        };
        Topic::filter_by_id(topic_id)
            .update()
            .summary(&combined)
            .exec(&mut db)
            .await?;
        Ok(topic)
    }

    pub async fn update_topic(
        &self,
        topic_id: Uuid,
        title: Option<&str>,
        summary: Option<&str>,
    ) -> Result<Topic> {
        let mut db = self.db.clone();
        let topic = Topic::get_by_id(&mut db, &topic_id).await?;
        let mut builder = Topic::filter_by_id(topic_id).update();
        if let Some(t) = title {
            builder = builder.title(t);
        }
        if let Some(s) = summary {
            builder = builder.summary(s);
        }
        builder.exec(&mut db).await?;
        Ok(topic)
    }

    pub async fn close_topic(&self, topic_id: Uuid, summary: &str) -> Result<Topic> {
        let mut db = self.db.clone();
        let topic = Topic::get_by_id(&mut db, &topic_id).await?;
        topic.ensure_not_closed()?;

        Topic::filter_by_id(topic_id)
            .update()
            .status("closed")
            .summary(summary)
            .closed_at(Some(jiff::Timestamp::now()))
            .exec(&mut db)
            .await?;

        pgvector::store_embedding(&*self.embedding, &self.pool, "topic", topic_id, summary).await?;

        Ok(topic)
    }

    pub async fn search_related_topics(
        &self,
        chat_id: ChatId,
        query: &[f32],
        limit: i64,
    ) -> Result<Vec<TopicSearchResult>> {
        let rows = pgvector::search_embedding_vec(
            &self.pool,
            "topic",
            query,
            chat_id.0,
            Some("status = 'closed'"),
            limit,
        )
        .await?;
        if rows.is_empty() {
            return Ok(Vec::new());
        }

        let ids: Vec<Uuid> = rows.iter().map(|(id, _)| *id).collect();
        let topics: Vec<Topic> = Topic::filter(Topic::fields().id().in_list(ids))
            .exec(&mut self.db.clone())
            .await?;

        let map: HashMap<Uuid, Topic> = topics.into_iter().map(|t| (t.id, t)).collect();

        Ok(rows
            .into_iter()
            .filter_map(|(id, dist)| {
                map.get(&id).map(|t| TopicSearchResult {
                    topic: t.clone(),
                    distance: dist,
                })
            })
            .collect())
    }

    pub async fn get_active_topics(&self, chat_id: ChatId) -> Result<Vec<Topic>> {
        Topic::filter(
            Topic::fields()
                .chat_id()
                .eq(chat_id.0)
                .and(Topic::fields().status().eq("active")),
        )
        .order_by(Topic::fields().last_active_at().desc())
        .exec(&mut self.db.clone())
        .await
        .map_err(Into::into)
    }

    pub async fn search_topics_by_query(
        &self,
        chat_id: ChatId,
        query: &str,
        limit: i64,
    ) -> Result<Vec<TopicSearchResult>> {
        let embedding = self.embedding.generate_embedding(query).await?;
        self.search_related_topics(chat_id, &embedding, limit).await
    }

    pub async fn list_topics(
        &self,
        chat_id: ChatId,
        status: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Topic>> {
        let mut q = Topic::filter(Topic::fields().chat_id().eq(chat_id.0));
        if let Some(s) = status {
            q = q.filter(Topic::fields().status().eq(s));
        }
        q.order_by(Topic::fields().last_active_at().desc())
            .limit(limit as usize)
            .offset(offset as usize)
            .exec(&mut self.db.clone())
            .await
            .map_err(Into::into)
    }

    pub async fn delete_topic(&self, topic_id: Uuid) -> Result<()> {
        Topic::delete_by_id(&mut self.db.clone(), topic_id).await?;
        if let Err(e) = pgvector::clear_embedding_vec(&self.pool, "topic", topic_id).await {
            tracing::warn!(topic_id = %topic_id, "Failed to clear topic embedding: {e}");
        }
        Ok(())
    }
}

async fn sync_topic_times(tx: &mut toasty::Transaction<'_>, topic_id: Uuid) -> Result<()> {
    let first = Message::filter(Message::fields().topic_id().eq(Some(topic_id)))
        .order_by((
            Message::fields().sent_at().asc(),
            Message::fields().created_at().asc(),
        ))
        .first()
        .exec(tx)
        .await?;
    let last = Message::filter(Message::fields().topic_id().eq(Some(topic_id)))
        .order_by((
            Message::fields().sent_at().desc(),
            Message::fields().created_at().desc(),
        ))
        .first()
        .exec(tx)
        .await?;

    if let Some(ref msg) = first {
        Topic::filter_by_id(topic_id)
            .update()
            .started_at(msg.active_at())
            .exec(tx)
            .await?;
    }
    if let Some(ref msg) = last {
        Topic::filter_by_id(topic_id)
            .update()
            .last_active_at(msg.active_at())
            .exec(tx)
            .await?;
    }
    Ok(())
}
