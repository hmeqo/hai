use anyhow::Result;
use pgvector::Vector;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{entity::Topic, vo::TopicSearchResult};

pub struct TopicRepo;

impl TopicRepo {
    /// 创建一个新话题
    pub async fn create(
        pool: &PgPool,
        chat_id: i64,
        title: &str,
        summary: &str,
        meta: Option<serde_json::Value>,
    ) -> Result<Topic> {
        let row = sqlx::query_as!(
            Topic,
            r#"
            INSERT INTO topic (chat_id, title, summary, meta, started_at, last_active_at)
            VALUES ($1, $2, $3, $4, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            RETURNING
                id as "id: Uuid",
                chat_id,
                title,
                summary,
                embedding as "embedding: Vector",
                status as "status!",
                parent_topic_id as "parent_topic_id: Uuid",
                token_count as "token_count!",
                message_count as "message_count!",
                meta,
                started_at as "started_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp",
                closed_at as "closed_at: jiff_sqlx::Timestamp",
                last_active_at as "last_active_at!: jiff_sqlx::Timestamp"
            "#,
            chat_id,
            title,
            summary,
            meta,
        )
        .fetch_one(pool)
        .await?;
        Ok(row)
    }

    /// 查询指定会话的所有活跃话题
    pub async fn list_active(pool: &PgPool, chat_id: i64) -> Result<Vec<Topic>> {
        let rows = sqlx::query_as!(
            Topic,
            r#"
            SELECT
                id as "id: Uuid",
                chat_id,
                title,
                summary,
                embedding as "embedding: Vector",
                status as "status!",
                parent_topic_id as "parent_topic_id: Uuid",
                token_count as "token_count!",
                message_count as "message_count!",
                meta,
                started_at as "started_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp",
                closed_at as "closed_at: jiff_sqlx::Timestamp",
                last_active_at as "last_active_at!: jiff_sqlx::Timestamp"
            FROM topic
            WHERE chat_id = $1 AND status = 'active'
            ORDER BY last_active_at DESC
            "#,
            chat_id,
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// 分页查询话题列表，支持按状态筛选
    pub async fn list_paged(
        pool: &PgPool,
        chat_id: i64,
        status: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Topic>> {
        let rows = sqlx::query_as!(
            Topic,
            r#"
            SELECT
                id as "id: Uuid",
                chat_id,
                title,
                summary,
                embedding as "embedding: Vector",
                status as "status!",
                parent_topic_id as "parent_topic_id: Uuid",
                token_count as "token_count!",
                message_count as "message_count!",
                meta,
                started_at as "started_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp",
                closed_at as "closed_at: jiff_sqlx::Timestamp",
                last_active_at as "last_active_at!: jiff_sqlx::Timestamp"
            FROM topic
            WHERE chat_id = $1 AND ($2::TEXT IS NULL OR status = $2)
            ORDER BY last_active_at DESC
            LIMIT $3 OFFSET $4
            "#,
            chat_id,
            status,
            limit,
            offset,
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// 通过 ID 查询话题
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Topic>> {
        let row = sqlx::query_as!(
            Topic,
            r#"
            SELECT
                id as "id: Uuid",
                chat_id,
                title,
                summary,
                embedding as "embedding: Vector",
                status as "status!",
                parent_topic_id as "parent_topic_id: Uuid",
                token_count as "token_count!",
                message_count as "message_count!",
                meta,
                started_at as "started_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp",
                closed_at as "closed_at: jiff_sqlx::Timestamp",
                last_active_at as "last_active_at!: jiff_sqlx::Timestamp"
            FROM topic
            WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// 更新话题标题
    pub async fn update_title(pool: &PgPool, id: Uuid, title: &str) -> Result<Option<Topic>> {
        let row = sqlx::query_as!(
            Topic,
            r#"
            UPDATE topic
            SET title = $1
            WHERE id = $2
            RETURNING
                id as "id: Uuid",
                chat_id,
                title,
                summary,
                embedding as "embedding: Vector",
                status as "status!",
                parent_topic_id as "parent_topic_id: Uuid",
                token_count as "token_count!",
                message_count as "message_count!",
                meta,
                started_at as "started_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp",
                closed_at as "closed_at: jiff_sqlx::Timestamp",
                last_active_at as "last_active_at!: jiff_sqlx::Timestamp"
            "#,
            title,
            id,
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// 追加话题摘要（保留原有内容）
    pub async fn append_summary(
        pool: &PgPool,
        id: Uuid,
        new_summary: &str,
    ) -> Result<Option<Topic>> {
        let row = sqlx::query_as!(
            Topic,
            r#"
            UPDATE topic
            SET
                summary = COALESCE(summary, '') || $1,
                updated_at = NOW()
            WHERE id = $2
            RETURNING
                id as "id: Uuid",
                chat_id,
                title,
                summary,
                embedding as "embedding: Vector",
                status as "status!",
                parent_topic_id as "parent_topic_id: Uuid",
                token_count as "token_count!",
                message_count as "message_count!",
                meta,
                started_at as "started_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp",
                closed_at as "closed_at: jiff_sqlx::Timestamp",
                last_active_at as "last_active_at!: jiff_sqlx::Timestamp"
            "#,
            new_summary,
            id,
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// 更新话题摘要（覆盖原有内容）
    pub async fn update_summary(
        pool: &PgPool,
        id: Uuid,
        new_summary: &str,
    ) -> Result<Option<Topic>> {
        let row = sqlx::query_as!(
            Topic,
            r#"
            UPDATE topic
            SET
                summary = $1,
                updated_at = NOW()
            WHERE id = $2
            RETURNING
                id as "id: Uuid",
                chat_id,
                title,
                summary,
                embedding as "embedding: Vector",
                status as "status!",
                parent_topic_id as "parent_topic_id: Uuid",
                token_count as "token_count!",
                message_count as "message_count!",
                meta,
                started_at as "started_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp",
                closed_at as "closed_at: jiff_sqlx::Timestamp",
                last_active_at as "last_active_at!: jiff_sqlx::Timestamp"
            "#,
            new_summary,
            id,
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// 完结话题并记录最终摘要
    pub async fn close_with_summary(
        pool: &PgPool,
        id: Uuid,
        summary: &str,
        embedding: Option<Vector>,
        token_count: i32,
        message_count: i32,
    ) -> Result<Option<Topic>> {
        let row = sqlx::query_as!(
            Topic,
            r#"
            UPDATE topic
            SET
                summary = $1,
                embedding = $2,
                status = 'closed',
                token_count = $3,
                message_count = $4,
                closed_at = CURRENT_TIMESTAMP
            WHERE id = $5
            RETURNING
                id as "id: Uuid",
                chat_id,
                title,
                summary,
                embedding as "embedding: Vector",
                status as "status!",
                parent_topic_id as "parent_topic_id: Uuid",
                token_count as "token_count!",
                message_count as "message_count!",
                meta,
                started_at as "started_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp",
                closed_at as "closed_at: jiff_sqlx::Timestamp",
                last_active_at as "last_active_at!: jiff_sqlx::Timestamp"
            "#,
            summary,
            embedding as Option<Vector>,
            token_count,
            message_count,
            id,
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// 删除话题
    pub async fn delete(pool: &PgPool, id: Uuid) -> Result<u64> {
        let result = sqlx::query!("DELETE FROM topic WHERE id = $1", id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// 向量相似度检索
    pub async fn search_by_embedding(
        pool: &PgPool,
        chat_id: i64,
        embedding: &Vector,
        limit: i64,
    ) -> Result<Vec<TopicSearchResult>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                id as "id: Uuid",
                chat_id,
                title,
                summary,
                embedding as "embedding: Vector",
                status as "status!",
                parent_topic_id as "parent_topic_id: Uuid",
                token_count as "token_count!",
                message_count as "message_count!",
                meta,
                started_at as "started_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp",
                closed_at as "closed_at: jiff_sqlx::Timestamp",
                last_active_at as "last_active_at!: jiff_sqlx::Timestamp",
                (embedding <=> $2) as "distance!: f64"
            FROM topic
            WHERE chat_id = $1 AND embedding IS NOT NULL
            ORDER BY embedding <=> $2
            LIMIT $3
            "#,
            chat_id,
            embedding as &Vector,
            limit,
        )
        .fetch_all(pool)
        .await?;

        let results = rows
            .into_iter()
            .map(|r| TopicSearchResult {
                distance: r.distance,
                topic: Topic {
                    id: r.id,
                    chat_id: r.chat_id,
                    title: r.title,
                    summary: r.summary,
                    embedding: r.embedding,
                    status: r.status,
                    parent_topic_id: r.parent_topic_id,
                    token_count: r.token_count,
                    message_count: r.message_count,
                    meta: r.meta,
                    started_at: r.started_at,
                    created_at: r.created_at,
                    updated_at: r.updated_at,
                    closed_at: r.closed_at,
                    last_active_at: r.last_active_at,
                },
            })
            .collect();
        Ok(results)
    }
}
