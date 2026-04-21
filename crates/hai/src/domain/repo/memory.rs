use anyhow::Result;
use pgvector::Vector;
use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{entity::Memory, vo::MemorySearchResult};

pub struct MemoryRepo;

impl MemoryRepo {
    /// 创建一条记忆
    pub async fn create(pool: &PgPool, memory: Memory) -> Result<Memory> {
        let row = sqlx::query_as!(
            Memory,
            r#"
            INSERT INTO memory (
                id, account_id, chat_id, type, content,
                embedding, importance, subject, "references", meta
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            RETURNING
                id,
                account_id,
                chat_id,
                type as "type_!",
                content as "content!",
                embedding as "embedding: Vector",
                importance as "importance!",
                subject,
                "references" as "references: serde_json::Value",
                meta,
                last_accessed_at as "last_accessed_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            "#,
            memory.id,
            memory.account_id,
            memory.chat_id,
            memory.type_,
            memory.content,
            memory.embedding as Option<Vector>,
            memory.importance,
            memory.subject,
            memory.references,
            memory.meta,
        )
        .fetch_one(pool)
        .await?;
        Ok(row)
    }

    /// 通过类型和会话查找记忆
    pub async fn search(
        pool: &PgPool,
        chat_id: i64,
        embedding: &Vector,
        limit: i64,
    ) -> Result<Vec<MemorySearchResult>> {
        let rows = sqlx::query!(
            r#"
            SELECT
                id,
                account_id,
                chat_id,
                type as "type_!",
                content as "content!",
                embedding as "embedding: Vector",
                importance as "importance!",
                subject,
                "references" as "references: serde_json::Value",
                meta,
                last_accessed_at as "last_accessed_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp",
                (embedding <=> $2) as "distance!: f64"
            FROM memory
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
            .map(|r| MemorySearchResult {
                distance: r.distance,
                memory: Memory {
                    id: r.id,
                    account_id: r.account_id,
                    chat_id: r.chat_id,
                    type_: r.type_,
                    content: r.content,
                    embedding: r.embedding,
                    importance: r.importance,
                    subject: r.subject,
                    references: r.references,
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
                type as "type_!",
                content as "content!",
                embedding as "embedding: Vector",
                importance as "importance!",
                subject,
                "references" as "references: serde_json::Value",
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

    /// 更新记忆内容或元数据
    pub async fn update(
        pool: &PgPool,
        id: Uuid,
        content: Option<&str>,
        importance: Option<i32>,
        meta: Option<serde_json::Value>,
        embedding: Option<Vector>,
        references: Option<serde_json::Value>,
    ) -> Result<Option<Memory>> {
        let row = sqlx::query_as!(
            Memory,
            r#"
            UPDATE memory
            SET content = COALESCE($1, content),
                importance = COALESCE($2, importance),
                meta = COALESCE($3, meta),
                embedding = COALESCE($4, embedding),
                "references" = COALESCE($5, "references"),
                updated_at = CURRENT_TIMESTAMP
            WHERE id = $6
            RETURNING
                id,
                account_id,
                chat_id,
                type as "type_!",
                content as "content!",
                embedding as "embedding: Vector",
                importance as "importance!",
                subject,
                "references" as "references: serde_json::Value",
                meta,
                last_accessed_at as "last_accessed_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            "#,
            content,
            importance,
            meta,
            embedding as Option<Vector>,
            references,
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

    /// 通过唯一键查找 UserFact（account_id + chat_id + content 完全匹配）
    pub async fn find_user_fact(
        pool: &PgPool,
        account_id: i64,
        chat_id: i64,
        content: &str,
    ) -> Result<Option<Memory>> {
        let row = sqlx::query_as!(
            Memory,
            r#"
            SELECT
                id,
                account_id,
                chat_id,
                type as "type_!",
                content as "content!",
                embedding as "embedding: Vector",
                importance as "importance!",
                subject,
                "references" as "references: serde_json::Value",
                meta,
                last_accessed_at as "last_accessed_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            FROM memory
            WHERE type = 'user_fact' AND account_id = $1 AND chat_id = $2 AND content = $3
            LIMIT 1
            "#,
            account_id,
            chat_id,
            content,
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// 通过唯一键查找 Knowledge（chat_id + content 完全匹配）
    pub async fn find_knowledge(
        pool: &PgPool,
        chat_id: i64,
        content: &str,
    ) -> Result<Option<Memory>> {
        let row = sqlx::query_as!(
            Memory,
            r#"
            SELECT
                id,
                account_id,
                chat_id,
                type as "type_!",
                content as "content!",
                embedding as "embedding: Vector",
                importance as "importance!",
                subject,
                "references" as "references: serde_json::Value",
                meta,
                last_accessed_at as "last_accessed_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            FROM memory
            WHERE type = 'knowledge' AND chat_id = $1 AND content = $2
            LIMIT 1
            "#,
            chat_id,
            content,
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// 通过唯一键查找 AgentNote（chat_id + content 完全匹配）
    pub async fn find_agent_note(
        pool: &PgPool,
        chat_id: i64,
        content: &str,
    ) -> Result<Option<Memory>> {
        let row = sqlx::query_as!(
            Memory,
            r#"
            SELECT
                id,
                account_id,
                chat_id,
                type as "type_!",
                content as "content!",
                embedding as "embedding: Vector",
                importance as "importance!",
                subject,
                "references" as "references: serde_json::Value",
                meta,
                last_accessed_at as "last_accessed_at!: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            FROM memory
            WHERE type = 'agent_note' AND chat_id = $1 AND content = $2
            LIMIT 1
            "#,
            chat_id,
            content,
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }
}
