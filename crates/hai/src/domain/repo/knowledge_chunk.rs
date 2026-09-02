use sqlx::PgPool;
use uuid::Uuid;

use crate::{domain::model::KnowledgeChunk, error::Result, util::pgvector};

#[derive(Debug, Clone)]
pub struct KnowledgeChunkRepo {
    pool: PgPool,
}

impl KnowledgeChunkRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// pgvector 向量检索，返回 top-k `(id, distance)`。
    /// 算子 `<->`（L2 距离，与 `pgvector::search_embedding_vec` 保持一致）。
    /// `document_ids: Some` = 文档白名单过滤（`= ANY($2)` 参数化绑定）。
    pub async fn search_by_embedding(
        &self,
        query: &[f32],
        document_ids: Option<&[Uuid]>,
        limit: i64,
    ) -> Result<Vec<(Uuid, f64)>> {
        let qv = pgvector::vec_to_pgstring(query);
        let mut sql = String::from(
            "SELECT id, embedding <-> $1::vector AS distance \
             FROM knowledge_chunk WHERE embedding IS NOT NULL",
        );
        let mut n = 1usize;
        if document_ids.is_some() {
            n += 1;
            sql.push_str(&format!(" AND document_id = ANY(${n})"));
        }
        n += 1;
        sql.push_str(&format!(" ORDER BY embedding <-> $1::vector LIMIT ${n}"));

        let mut q = sqlx::query_as::<_, (Uuid, f64)>(sqlx::AssertSqlSafe(sql.as_str())).bind(qv);
        if let Some(ids) = document_ids {
            q = q.bind(ids);
        }
        let rows = q.bind(limit).fetch_all(&self.pool).await?;
        Ok(rows)
    }

    /// 按 id 批量取块（检索结果溯源用）。
    pub async fn by_ids(&self, ids: &[Uuid]) -> Result<Vec<KnowledgeChunk>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows =
            sqlx::query_as::<_, KnowledgeChunk>("SELECT * FROM knowledge_chunk WHERE id = ANY($1)")
                .bind(ids)
                .fetch_all(&self.pool)
                .await?;
        Ok(rows)
    }
}
