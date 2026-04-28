use pgvector::Vector;
use sqlx::PgPool;
use uuid::Uuid;

use crate::{domain::entity::Perception, error::Result};

pub struct PerceptionRepo;

impl PerceptionRepo {
    /// 查找已有的分析结果（按 source JSONB 精确匹配）
    pub async fn find(
        pool: &PgPool,
        source: &serde_json::Value,
        parser: &str,
        prompt: Option<&str>,
    ) -> Result<Option<Perception>> {
        let row = sqlx::query_as!(
            Perception,
            r#"
            SELECT id, source as "source: serde_json::Value", parser, prompt, content,
                embedding as "embedding: Vector",
                created_at as "created_at: jiff_sqlx::Timestamp"
            FROM perception
            WHERE source = $1 AND parser = $2
            AND (prompt IS NOT DISTINCT FROM $3)
            "#,
            source,
            parser,
            prompt,
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }

    /// 批量按 file_id 查询平台附件的 perception 结果。
    /// 返回 `(file_id, Perception)` 列表。
    pub async fn find_by_platform_file_ids(
        pool: &PgPool,
        file_ids: &[String],
    ) -> Result<Vec<(String, Perception)>> {
        if file_ids.is_empty() {
            return Ok(Vec::new());
        }

        let rows = sqlx::query!(
            r#"
            SELECT
                p.source->>'file_id' as "file_id!",
                p.id as "id!: Uuid",
                p.source as "source: serde_json::Value",
                p.parser as "parser!",
                p.prompt,
                p.content as "content!",
                p.embedding as "embedding: Vector",
                p.created_at as "created_at!: jiff_sqlx::Timestamp"
            FROM perception p
            WHERE p.source->>'type' = 'platform'
              AND p.source->>'file_id' = ANY($1)
            "#,
            file_ids,
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.file_id,
                    Perception {
                        id: r.id,
                        source: r.source,
                        parser: r.parser,
                        prompt: r.prompt,
                        content: r.content,
                        embedding: r.embedding,
                        created_at: r.created_at,
                    },
                )
            })
            .collect())
    }

    /// 批量查询一组 URL 对应的 perception 结果。
    pub async fn find_by_urls(pool: &PgPool, urls: &[String]) -> Result<Vec<Perception>> {
        if urls.is_empty() {
            return Ok(Vec::new());
        }

        let rows = sqlx::query_as!(
            Perception,
            r#"
            SELECT id, source as "source: serde_json::Value", parser, prompt, content,
                embedding as "embedding: Vector",
                created_at as "created_at: jiff_sqlx::Timestamp"
            FROM perception
            WHERE source->>'type' = 'url'
              AND source->>'url' = ANY($1)
            "#,
            urls,
        )
        .fetch_all(pool)
        .await?;

        Ok(rows)
    }

    /// 创建或更新分析结果
    pub async fn upsert(
        pool: &PgPool,
        source: &serde_json::Value,
        parser: &str,
        prompt: Option<&str>,
        content: &str,
    ) -> Result<Perception> {
        let row = if let Some(prompt) = prompt {
            sqlx::query_as!(
                Perception,
                r#"
                INSERT INTO perception (source, parser, prompt, content)
                VALUES ($1, $2, $3, $4)
                ON CONFLICT (source, parser, prompt)
                WHERE prompt IS NOT NULL
                DO UPDATE SET content = EXCLUDED.content
                RETURNING id, source as "source: serde_json::Value", parser, prompt, content,
                    embedding as "embedding: Vector",
                    created_at as "created_at: jiff_sqlx::Timestamp"
                "#,
                source,
                parser,
                prompt,
                content,
            )
            .fetch_one(pool)
            .await?
        } else {
            sqlx::query_as!(
                Perception,
                r#"
                INSERT INTO perception (source, parser, prompt, content)
                VALUES ($1, $2, NULL, $3)
                ON CONFLICT (source, parser)
                WHERE prompt IS NULL
                DO UPDATE SET content = EXCLUDED.content
                RETURNING id, source as "source: serde_json::Value", parser, prompt, content,
                    embedding as "embedding: Vector",
                    created_at as "created_at: jiff_sqlx::Timestamp"
                "#,
                source,
                parser,
                content,
            )
            .fetch_one(pool)
            .await?
        };
        Ok(row)
    }

    pub async fn set_embedding(
        pool: &PgPool,
        id: Uuid,
        embedding: &pgvector::Vector,
    ) -> Result<()> {
        sqlx::query!(
            r#"
            UPDATE perception SET embedding = $1 WHERE id = $2
            "#,
            embedding as &pgvector::Vector,
            id,
        )
        .execute(pool)
        .await?;
        Ok(())
    }
}
