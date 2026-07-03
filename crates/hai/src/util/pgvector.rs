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
pub async fn ensure_embedding_schema(db: &mut toasty::Db, dimension: i32) -> Result<()> {
    toasty::sql::statement("CREATE EXTENSION IF NOT EXISTS vector")
        .exec(db)
        .await?;

    for table in &["memory", "topic", "perception"] {
        toasty::sql::statement(format!(
            "ALTER TABLE {table} ADD COLUMN IF NOT EXISTS embedding vector({dimension})"
        ))
        .exec(db)
        .await?;
        toasty::sql::statement(format!(
            "ALTER TABLE {table} ALTER COLUMN embedding TYPE vector({dimension}) USING embedding::vector"
        ))
        .exec(db)
        .await?;
    }
    Ok(())
}

/// pgvector 余弦距离搜索，返回 top-k `(id, distance)`。
/// `extra_filter` 为额外 WHERE 条件（不含 `chat_id`，后者已参数化）。
pub async fn search_embedding_vec(
    pool: &sqlx::PgPool,
    table: &str,
    query: &[f32],
    chat_id: i64,
    extra_filter: Option<&str>,
    limit: i64,
) -> Result<Vec<(Uuid, f64)>> {
    let qv = vec_to_pgstring(query);
    let where_clause = match extra_filter {
        Some(f) => format!("chat_id = $3 AND {f} AND embedding IS NOT NULL"),
        None => "chat_id = $3 AND embedding IS NOT NULL".to_string(),
    };
    let sql = format!(
        "SELECT id, embedding <-> $1::vector AS distance \
         FROM {table} \
         WHERE {where_clause} \
         ORDER BY embedding <-> $1::vector \
         LIMIT $2"
    );
    Ok(sqlx::query_as(&sql)
        .bind(qv)
        .bind(limit)
        .bind(chat_id)
        .fetch_all(pool)
        .await?)
}

/// 写入或更新单条 embedding。
pub async fn upsert_embedding_vec(
    pool: &sqlx::PgPool,
    table: &str,
    id: Uuid,
    emb: &[f32],
) -> Result<()> {
    let sql = format!("UPDATE {table} SET embedding = $1::vector WHERE id = $2");
    sqlx::query(&sql)
        .bind(vec_to_pgstring(emb))
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// 清空单条 embedding。
pub async fn clear_embedding_vec(pool: &sqlx::PgPool, table: &str, id: Uuid) -> Result<()> {
    let sql = format!("UPDATE {table} SET embedding = NULL WHERE id = $1");
    sqlx::query(&sql).bind(id).execute(pool).await?;
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
