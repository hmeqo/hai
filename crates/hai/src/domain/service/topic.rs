use std::{collections::HashMap, sync::Arc};

use uuid::Uuid;

use crate::{
    agentcore::embedding::EmbeddingService,
    domain::{
        model::Topic,
        repo::Repos,
        vo::{ChatId, MessageId, TopicId, TopicSearchResult},
    },
    error::{ErrorKind, Result},
    util::pgvector,
};

#[derive(Debug)]
pub struct TopicService {
    repos: Repos,
    embedding: Arc<dyn EmbeddingService>,
}

impl TopicService {
    pub fn new(repos: Repos, embedding: Arc<dyn EmbeddingService>) -> Self {
        Self { repos, embedding }
    }

    pub async fn create_topic(
        &self,
        chat_id: ChatId,
        title: &str,
        summary: &str,
        message_ids: &[MessageId],
        meta: Option<serde_json::Value>,
    ) -> Result<Topic> {
        self.repos
            .topic
            .create_with_messages(
                chat_id.0,
                title,
                summary,
                None,
                &MessageId::raw_ids(message_ids),
                meta,
            )
            .await
    }

    pub async fn assign_topic(&self, message_ids: &[MessageId], topic_id: TopicId) -> Result<()> {
        let topic = self.get_topic_or_err(topic_id.0).await?;
        topic.ensure_not_closed()?;
        self.repos
            .topic
            .assign_messages(&MessageId::raw_ids(message_ids), topic_id.0)
            .await
    }

    pub async fn append_summary(&self, topic_id: TopicId, new_summary: &str) -> Result<Topic> {
        let topic = self.get_topic_or_err(topic_id.0).await?;
        topic.ensure_not_closed()?;

        let formatted = format!("\n---\n{new_summary}");
        let combined = match &topic.summary {
            Some(s) => format!("{s}{formatted}"),
            None => formatted,
        };
        self.repos
            .topic
            .update_fields(topic_id.0, None, Some(&combined))
            .await?;
        Ok(topic)
    }

    pub async fn update_topic(
        &self,
        topic_id: TopicId,
        title: Option<&str>,
        summary: Option<&str>,
    ) -> Result<Topic> {
        let topic = self.get_topic_or_err(topic_id.0).await?;
        self.repos
            .topic
            .update_fields(topic_id.0, title, summary)
            .await?;
        Ok(topic)
    }

    pub async fn close_topic(&self, topic_id: TopicId, summary: &str) -> Result<Topic> {
        let topic = self.get_topic_or_err(topic_id.0).await?;
        topic.ensure_not_closed()?;

        self.repos.topic.close(topic_id.0, summary).await?;

        if let Err(e) = pgvector::store_embedding(
            &*self.embedding,
            self.repos.pool(),
            "topic",
            topic_id.0,
            summary,
        )
        .await
        {
            tracing::warn!(topic_id = %topic_id, "close_topic embedding failed: {e}");
        }

        Ok(topic)
    }

    pub async fn search_related_topics(
        &self,
        chat_id: ChatId,
        query: &[f32],
        limit: i64,
    ) -> Result<Vec<TopicSearchResult>> {
        let rows = pgvector::search_embedding_vec(
            self.repos.pool(),
            "topic",
            query,
            Some(chat_id.0),
            Some("status = 'closed'"),
            limit,
        )
        .await?;
        self.rows_to_results(rows).await
    }

    /// 向量检索行 → 结果（补 topic 实体并带距离）。
    async fn rows_to_results(&self, rows: Vec<(Uuid, f64)>) -> Result<Vec<TopicSearchResult>> {
        if rows.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<Uuid> = rows.iter().map(|(id, _)| *id).collect();
        let topics: Vec<Topic> = self.repos.topic.by_ids(&ids).await?;
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
        self.repos.topic.active_by_chat(chat_id.0).await
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

    /// 检索话题：query 有 → 语义检索（+时间过滤）；无 → 按时间范围列出。
    pub async fn search_topics(
        &self,
        chat_id: ChatId,
        query: Option<&str>,
        since: Option<jiff::Timestamp>,
        until: Option<jiff::Timestamp>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TopicSearchResult>> {
        let limit = limit.max(1) as usize;
        if let Some(q) = query {
            // 时间条件下推到 SQL（`search_embedding_vec_time`）：避免先取 top-N 再
            // 内存过滤导致窗内命中被截断。
            let embedding = self.embedding.generate_embedding(q).await?;
            let range = match (since, until) {
                (Some(s), Some(u)) => Some((s, u)),
                (Some(s), None) => Some((s, jiff::Timestamp::now())),
                (None, Some(u)) => Some((jiff::Timestamp::UNIX_EPOCH, u)),
                (None, None) => None,
            };
            let rows = pgvector::search_embedding_vec_time(
                self.repos.pool(),
                "topic",
                &embedding,
                chat_id.0,
                range,
                limit as i64,
                offset,
            )
            .await?;
            return self.rows_to_results(rows).await;
        }
        let topics = self
            .repos
            .topic
            .by_chat_time(chat_id.0, since, until, limit as i64, offset)
            .await?;
        Ok(topics
            .into_iter()
            .map(|t| TopicSearchResult {
                topic: t,
                distance: 0.0,
            })
            .collect())
    }

    pub async fn delete_topic(&self, topic_id: TopicId) -> Result<()> {
        self.repos.topic.delete_by_id(topic_id.0).await?;
        if let Err(e) = pgvector::clear_embedding_vec(self.repos.pool(), "topic", topic_id.0).await
        {
            tracing::warn!(topic_id = %topic_id, "Failed to clear topic embedding: {e}");
        }
        Ok(())
    }

    async fn get_topic_or_err(&self, id: Uuid) -> Result<Topic> {
        self.repos
            .topic
            .by_id(id)
            .await?
            .ok_or_else(|| ErrorKind::NotFound.msg(format!("Topic not found: {id}")))
    }
}
