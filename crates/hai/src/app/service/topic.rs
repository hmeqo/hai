use std::sync::Arc;

use anyhow::Result;
use pgvector::Vector;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    agent::multimodal::EmbeddingService,
    app::{
        domain::entity::{Topic, TopicStatus},
        repo::{MessageRepo, TopicRepo},
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
        title: Option<&str>,
        message_ids: &[i64],
        meta: Option<serde_json::Value>,
    ) -> Result<Topic> {
        let topic = TopicRepo::create(&self.pool, chat_id, title, meta).await?;
        if !message_ids.is_empty() {
            MessageRepo::assign_topic(&self.pool, message_ids, topic.id).await?;
        }
        Ok(topic)
    }

    /// 将消息批量关联到已存在的话题并标记为已处理
    pub async fn assign_topic(&self, message_ids: &[i64], topic_id: Uuid) -> Result<u64> {
        MessageRepo::assign_topic(&self.pool, message_ids, topic_id).await
    }

    /// 仅标记消息为已阅
    pub async fn mark_as_seen(&self, message_ids: &[i64]) -> Result<u64> {
        MessageRepo::mark_seen(&self.pool, message_ids).await
    }

    /// 标记消息为已回复
    pub async fn mark_as_replied(&self, message_ids: &[i64]) -> Result<u64> {
        MessageRepo::mark_replied(&self.pool, message_ids).await
    }

    /// 更新话题标题
    pub async fn update_title(&self, topic_id: Uuid, new_title: &str) -> Result<Option<Topic>> {
        TopicRepo::update_title(&self.pool, topic_id, new_title).await
    }

    /// 更新话题状态
    pub async fn update_status(
        &self,
        topic_id: Uuid,
        status: TopicStatus,
    ) -> Result<Option<Topic>> {
        TopicRepo::update_status(&self.pool, topic_id, status).await
    }

    /// 更新话题摘要（自动生成向量）
    pub async fn update_summary(&self, topic_id: Uuid, summary: &str) -> Result<Option<Topic>> {
        let embedding = self.embedding.generate_embedding(summary).await?;
        TopicRepo::update_summary(&self.pool, topic_id, summary, Some(Vector::from(embedding)))
            .await
    }

    /// 完结话题：更新摘要、向量嵌入，将状态设置为 closed
    pub async fn finish_topic(&self, topic_id: Uuid, summary: &str) -> Result<Option<Topic>> {
        // 自动计算该话题的消息数和 token 总数
        let stats = sqlx::query!(
            r#"
            SELECT
                COUNT(*)::INT as "message_count!",
                COALESCE(SUM(token_count), 0)::INT as "token_count!"
            FROM message
            WHERE topic_id = $1
            "#,
            topic_id
        )
        .fetch_one(&self.pool)
        .await?;

        let embedding = self.embedding.generate_embedding(summary).await?;
        TopicRepo::close_with_summary(
            &self.pool,
            topic_id,
            summary,
            Some(Vector::from(embedding)),
            stats.token_count,
            stats.message_count,
        )
        .await
    }

    /// 通过向量语义检索相关话题
    pub async fn search_topics(
        &self,
        chat_id: i64,
        query_embedding: &Vector,
        limit: i64,
    ) -> Result<Vec<crate::app::domain::model::TopicSearchResult>> {
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
    ) -> Result<Vec<crate::app::domain::model::TopicSearchResult>> {
        let embedding = self.embedding.generate_embedding(query).await?;
        let vector = pgvector::Vector::from(embedding);
        self.search_topics(chat_id, &vector, limit).await
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
