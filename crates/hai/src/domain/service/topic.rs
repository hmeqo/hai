use std::sync::Arc;

use anyhow::Result;
use pgvector::Vector;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    agent::multimodal::EmbeddingService,
    domain::{
        entity::{Topic, TopicStatus},
        repo::{MessageRepo, TopicRepo},
        vo::TopicSearchResult,
    },
};

/// 话题管理服务（对应 Agent A 工具集）
pub struct TopicService {
    pool: PgPool,
    embedding: Arc<EmbeddingService>,
}

impl TopicService {
    pub fn new(pool: PgPool, embedding: Arc<EmbeddingService>) -> Self {
        Self { pool, embedding }
    }

    /// 创建新话题，将指定消息关联到该话题并标记为已处理
    pub async fn create_topic(
        &self,
        chat_id: i64,
        title: &str,
        summary: &str,
        message_ids: &[i64],
        meta: Option<serde_json::Value>,
    ) -> Result<Topic> {
        let topic = TopicRepo::create(&self.pool, chat_id, title, summary, meta).await?;
        if !message_ids.is_empty() {
            MessageRepo::assign_topic(&self.pool, message_ids, topic.id).await?;
        }
        Ok(topic)
    }

    /// 将消息批量关联到已存在的话题并标记为已处理
    pub async fn assign_topic(&self, message_ids: &[i64], topic_id: Uuid) -> Result<u64> {
        MessageRepo::assign_topic(&self.pool, message_ids, topic_id).await
    }

    /// 标记消息为已回复
    pub async fn mark_as_replied(&self, message_ids: &[i64]) -> Result<u64> {
        MessageRepo::mark_replied(&self.pool, message_ids).await
    }

    /// 更新话题标题
    pub async fn update_title(&self, topic_id: Uuid, new_title: &str) -> Result<Option<Topic>> {
        TopicRepo::update_title(&self.pool, topic_id, new_title).await
    }

    /// 追加话题摘要（保留原有内容，追加新内容）
    pub async fn push_summary(&self, topic_id: Uuid, new_summary: &str) -> Result<Option<Topic>> {
        let topic = TopicRepo::find_by_id(&self.pool, topic_id).await?;
        if let Some(topic) = &topic
            && topic.status() == TopicStatus::Closed
        {
            anyhow::bail!("Cannot push summary to a closed topic");
        }
        let formatted = format!("\n---\n{}", new_summary);
        TopicRepo::append_summary(&self.pool, topic_id, &formatted).await
    }

    pub async fn update_summary(&self, topic_id: Uuid, new_summary: &str) -> Result<Option<Topic>> {
        let topic = TopicRepo::find_by_id(&self.pool, topic_id).await?;
        if let Some(topic) = &topic
            && topic.status() == TopicStatus::Closed
        {
            anyhow::bail!("Cannot update summary of a closed topic");
        }
        TopicRepo::update_summary(&self.pool, topic_id, new_summary).await
    }

    /// 完结话题：写入最终摘要、关闭话题
    pub async fn finish_topic(&self, topic_id: Uuid, summary: &str) -> Result<Option<Topic>> {
        let embedding = self.embedding.generate_embedding(summary).await?;
        TopicRepo::close_with_summary(
            &self.pool,
            topic_id,
            summary,
            Some(Vector::from(embedding)),
            0,
            0,
        )
        .await
    }

    /// 通过向量语义检索相关话题
    pub async fn search_related_topics(
        &self,
        chat_id: i64,
        query_embedding: &Vector,
        limit: i64,
    ) -> Result<Vec<TopicSearchResult>> {
        TopicRepo::search_by_embedding(&self.pool, chat_id, query_embedding, limit).await
    }

    /// 获取活跃话题
    pub async fn get_active_topics(&self, chat_id: i64) -> Result<Vec<Topic>> {
        TopicRepo::list_active(&self.pool, chat_id).await
    }

    /// 语义搜索话题
    pub async fn search_topics_by_query(
        &self,
        chat_id: i64,
        query: &str,
        limit: i64,
    ) -> Result<Vec<TopicSearchResult>> {
        let embedding = self.embedding.generate_embedding(query).await?;
        let vector = pgvector::Vector::from(embedding);
        self.search_related_topics(chat_id, &vector, limit).await
    }

    /// 分页获取话题列表
    pub async fn list_topics(
        &self,
        chat_id: i64,
        status: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Topic>> {
        TopicRepo::list_paged(&self.pool, chat_id, status, limit, offset).await
    }

    /// 删除话题
    pub async fn delete_topic(&self, topic_id: Uuid) -> Result<u64> {
        TopicRepo::delete(&self.pool, topic_id).await
    }
}
