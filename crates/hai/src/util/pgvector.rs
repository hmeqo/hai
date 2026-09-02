use uuid::Uuid;

use crate::{agentcore::embedding::EmbeddingService, error::Result};

/// 将 `&[f32]` 格式化为 pgvector 字面量 `[0.1,0.2,...]`。
pub fn vec_to_pgstring(v: &[f32]) -> String {
    format!(
        "[{}]",
        v.iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}

/// 确保 pgvector extension + embedding schema 就绪。
/// 可被 `db migrate` 和 `db rebuild embeddings` 共用。
pub async fn ensure_embedding_schema(pool: &sqlx::PgPool, dimension: i32) -> Result<()> {
    sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
        .execute(pool)
        .await?;

    for table in &["memory", "topic", "perception", "knowledge_chunk"] {
        sqlx::query(sqlx::AssertSqlSafe(
            format!("ALTER TABLE {table} ADD COLUMN IF NOT EXISTS embedding vector({dimension})")
                .as_str(),
        ))
        .execute(pool)
        .await?;
        // 维度变更（已有数据）时 cast 会失败——降级 warn，数据保持旧维度（rebuild 重建）
        if let Err(e) = sqlx::query(sqlx::AssertSqlSafe(format!(
            "ALTER TABLE {table} ALTER COLUMN embedding TYPE vector({dimension}) USING embedding::vector"
        ).as_str()))
        .execute(pool)
        .await
        {
            tracing::warn!(%table, %dimension, "embedding dimension cast failed: {e}");
        }
    }
    Ok(())
}

/// pgvector 余弦距离搜索，返回 top-k `(id, distance)`。
/// `chat_id: None` = 全局检索（不按 chat 过滤，如 knowledge_chunk）。
/// `extra_filter` 为额外 WHERE 条件（常量 SQL 片段，**禁止插值用户输入**）。
pub async fn search_embedding_vec(
    pool: &sqlx::PgPool,
    table: &str,
    query: &[f32],
    chat_id: Option<i64>,
    extra_filter: Option<&str>,
    limit: i64,
) -> Result<Vec<(Uuid, f64)>> {
    let qv = vec_to_pgstring(query);
    let mut where_parts: Vec<&str> = Vec::new();
    if chat_id.is_some() {
        where_parts.push("chat_id = $3");
    }
    if let Some(f) = extra_filter {
        where_parts.push(f);
    }
    where_parts.push("embedding IS NOT NULL");
    let where_clause = where_parts.join(" AND ");
    let sql = format!(
        "SELECT id, embedding <-> $1::vector AS distance \
         FROM {table} \
         WHERE {where_clause} \
         ORDER BY embedding <-> $1::vector \
         LIMIT $2"
    );
    let mut q = sqlx::query_as(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(qv)
        .bind(limit);
    if let Some(chat_id) = chat_id {
        q = q.bind(chat_id);
    }
    Ok(q.fetch_all(pool).await?)
}

/// 带时间范围过滤的向量检索（topic.search_topics 专用：时间条件下推，避免
/// "先取 top-N 再内存过滤"导致的窗内命中截断）。恒含 chat_id（$3）+ closed-only；
/// since（$4）/ until（$5）可选，与 SQL 条件同步绑定。
pub async fn search_embedding_vec_time(
    pool: &sqlx::PgPool,
    table: &str,
    query: &[f32],
    chat_id: i64,
    range: Option<(jiff::Timestamp, jiff::Timestamp)>,
    limit: i64,
    offset: i64,
) -> Result<Vec<(Uuid, f64)>> {
    let qv = vec_to_pgstring(query);
    let mut conds = vec!["chat_id = $3", "embedding IS NOT NULL", "status = 'closed'"];
    let mut binds: Vec<jiff_sqlx::Timestamp> = Vec::new();
    if let Some((since, until)) = range {
        conds.push("last_active_at >= $4");
        conds.push("last_active_at <= $5");
        binds.push(jiff_sqlx::Timestamp::from(since));
        binds.push(jiff_sqlx::Timestamp::from(until));
    }
    let offset_n = 3 + binds.len() + 1;
    let sql = format!(
        "SELECT id, embedding <-> $1::vector AS distance \
         FROM {table} \
         WHERE {} \
         ORDER BY embedding <-> $1::vector \
         LIMIT $2 OFFSET ${offset_n}",
        conds.join(" AND ")
    );
    let mut q = sqlx::query_as::<_, (Uuid, f64)>(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(qv)
        .bind(limit)
        .bind(chat_id);
    for b in binds {
        q = q.bind(b);
    }
    q = q.bind(offset);
    Ok(q.fetch_all(pool).await?)
}

pub async fn upsert_embedding_vec(
    pool: &sqlx::PgPool,
    table: &str,
    id: Uuid,
    emb: &[f32],
) -> Result<()> {
    let sql = format!("UPDATE {table} SET embedding = $1::vector WHERE id = $2");
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(vec_to_pgstring(emb))
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn clear_embedding_vec(pool: &sqlx::PgPool, table: &str, id: Uuid) -> Result<()> {
    let sql = format!("UPDATE {table} SET embedding = NULL WHERE id = $1");
    sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// 一站式：生成 embedding + 写入 DB。
pub async fn store_embedding(
    embedding: &dyn EmbeddingService,
    pool: &sqlx::PgPool,
    table: &str,
    id: Uuid,
    content: &str,
) -> Result<()> {
    let emb = embedding.generate_embedding(content).await?;
    upsert_embedding_vec(pool, table, id, &emb).await?;
    Ok(())
}
