use uuid::Uuid;

use crate::error::Result;

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

/// pgvector 余弦距离搜索，返回 top-k `(id, distance)`。
pub async fn search_embedding_vec(
    pool: &sqlx::PgPool,
    table: &str,
    query: &[f32],
    filter: &str,
    limit: i64,
) -> Result<Vec<(Uuid, f64)>> {
    let qv = vec_to_pgstring(query);
    let sql = format!(
        "SELECT id, embedding <-> $1::vector AS distance \
         FROM {table} \
         WHERE {filter} AND embedding IS NOT NULL \
         ORDER BY embedding <-> $1::vector \
         LIMIT $2"
    );
    Ok(sqlx::query_as(&sql)
        .bind(qv)
        .bind(limit)
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
