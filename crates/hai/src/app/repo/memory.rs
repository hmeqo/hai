use anyhow::Result;
use pgvector::Vector;
use sqlx::PgPool;
use uuid::Uuid;

use crate::app::domain::{entity::Memory, model::MemorySearchResult};

pub struct MemoryRepo;

impl MemoryRepo {
    /// 创建一条记忆
    pub async fn create(pool: &PgPool, memory: Memory) -> Result<Memory> {
        let row = sqlx::query_as!(
            Memory,
            r#"
            INSERT INTO memory (
                id, account_id, chat_id, topic_id, type, content,
                embedding, source_message_id, importance, meta
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING
                id,
                account_id,
                chat_id,
                topic_id as "topic_id: Uuid",
                type as "type_!",
                content as "content!",
                embedding as "embedding: Vector",
                source_message_id,
                importance as "importance!",
                meta,
                last_accessed_at as "last_accessed_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            "#,
            memory.id,
            memory.account_id,
            memory.chat_id,
            memory.topic_id,
            memory.type_,
            memory.content,
            memory.embedding as Option<Vector>,
            memory.source_message_id,
            memory.importance,
            memory.meta,
        )
        .fetch_one(pool)
        .await?;
        Ok(row)
    }

    /// 通过 ID 查找记忆
    pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<Memory>> {
        let row = sqlx::query_as!(
            Memory,
            r#"
            SELECT
                id,
                account_id,
                chat_id,
                topic_id as "topic_id: Uuid",
                type as "type_!",
                content as "content!",
                embedding as "embedding: Vector",
                source_message_id,
                importance as "importance!",
                meta,
                last_accessed_at as "last_accessed_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            FROM memory
            WHERE id = $1
            "#,
            id,
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// 向量检索记忆
    pub async fn search(
        pool: &PgPool,
        account_ids: &[i64],
        chat_id: Option<i64>,
        embedding: &Vector,
        limit: i64,
    ) -> Result<Vec<MemorySearchResult>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                id,
                account_id,
                chat_id,
                topic_id as "topic_id: Uuid",
                type as "type_!",
                content as "content!",
                embedding as "embedding: Vector",
                source_message_id,
                importance as "importance!",
                meta,
                last_accessed_at as "last_accessed_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp",
                (embedding <=> $3) as "distance!: f64"
            FROM memory
            WHERE (account_id = ANY($1) OR chat_id = $2) AND embedding IS NOT NULL
            ORDER BY embedding <=> $3
            LIMIT $4
            "#,
            account_ids,
            chat_id,
            embedding as &Vector,
            limit,
        )
        .fetch_all(pool)
        .await?;

        let results = rows
            .into_iter()
            .map(|r| MemorySearchResult {
                distance: r.distance,
                memory: Memory {
                    id: r.id,
                    account_id: r.account_id,
                    chat_id: r.chat_id,
                    topic_id: r.topic_id,
                    type_: r.type_,
                    content: r.content,
                    embedding: r.embedding,
                    source_message_id: r.source_message_id,
                    importance: r.importance,
                    meta: r.meta,
                    last_accessed_at: r.last_accessed_at,
                    created_at: r.created_at,
                    updated_at: r.updated_at,
                },
            })
            .collect();
        Ok(results)
    }

    /// 通过类型和会话查找记忆
    pub async fn find_by_type_and_chat(
        pool: &PgPool,
        type_: &str,
        chat_id: i64,
    ) -> Result<Option<Memory>> {
        let row = sqlx::query_as!(
            Memory,
            r#"
            SELECT
                id,
                account_id,
                chat_id,
                topic_id as "topic_id: Uuid",
                type as "type_!",
                content as "content!",
                embedding as "embedding: Vector",
                source_message_id,
                importance as "importance!",
                meta,
                last_accessed_at as "last_accessed_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            FROM memory
            WHERE type = $1 AND chat_id = $2
            LIMIT 1
            "#,
            type_,
            chat_id,
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// 批量获取话题相关的记忆
    pub async fn list_by_topic_ids(pool: &PgPool, topic_ids: &[Uuid]) -> Result<Vec<Memory>> {
        let rows = sqlx::query_as!(
            Memory,
            r#"
            SELECT
                id,
                account_id,
                chat_id,
                topic_id as "topic_id: Uuid",
                type as "type_!",
                content as "content!",
                embedding as "embedding: Vector",
                source_message_id,
                importance as "importance!",
                meta,
                last_accessed_at as "last_accessed_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            FROM memory
            WHERE topic_id = ANY($1)
            "#,
            topic_ids,
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// 批量获取消息相关的记忆
    pub async fn list_by_message_ids(pool: &PgPool, message_ids: &[i64]) -> Result<Vec<Memory>> {
        let rows = sqlx::query_as!(
            Memory,
            r#"
            SELECT
                id,
                account_id,
                chat_id,
                topic_id as "topic_id: Uuid",
                type as "type_!",
                content as "content!",
                embedding as "embedding: Vector",
                source_message_id,
                importance as "importance!",
                meta,
                last_accessed_at as "last_accessed_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            FROM memory
            WHERE source_message_id = ANY($1)
            "#,
            message_ids,
        )
        .fetch_all(pool)
        .await?;
        Ok(rows)
    }

    /// 更新记忆内容或元数据
    pub async fn update(
        pool: &PgPool,
        id: Uuid,
        content: Option<&str>,
        importance: Option<i32>,
        meta: Option<serde_json::Value>,
        embedding: Option<Vector>,
    ) -> Result<Option<Memory>> {
        let row = sqlx::query_as!(
            Memory,
            r#"
            UPDATE memory
            SET content = COALESCE($1, content),
                importance = COALESCE($2, importance),
                meta = COALESCE($3, meta),
                embedding = COALESCE($4, embedding),
                updated_at = CURRENT_TIMESTAMP
            WHERE id = $5
            RETURNING
                id,
                account_id,
                chat_id,
                topic_id as "topic_id: Uuid",
                type as "type_!",
                content as "content!",
                embedding as "embedding: Vector",
                source_message_id,
                importance as "importance!",
                meta,
                last_accessed_at as "last_accessed_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            "#,
            content,
            importance,
            meta,
            embedding as Option<Vector>,
            id,
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// 删除记忆
    pub async fn delete(pool: &PgPool, id: Uuid) -> Result<u64> {
        let result = sqlx::query!("DELETE FROM memory WHERE id = $1", id)
            .execute(pool)
            .await?;
        Ok(result.rows_affected())
    }
}
