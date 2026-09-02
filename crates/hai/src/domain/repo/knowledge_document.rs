use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    domain::model::{KnowledgeChunk, KnowledgeDocument},
    error::Result,
    util::pgvector,
};

#[derive(Debug, Clone)]
pub struct KnowledgeDocumentRepo {
    pool: PgPool,
}

impl KnowledgeDocumentRepo {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 按 source 查文档（source 唯一，幂等导入比对依据）。
    pub async fn find_by_source(&self, source: &str) -> Result<Option<KnowledgeDocument>> {
        sqlx::query_as::<_, KnowledgeDocument>(
            "SELECT * FROM knowledge_document WHERE source = $1 LIMIT 1",
        )
        .bind(source)
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::into)
    }

    /// 文档列表（可按 collection 过滤，更新时间倒序）。
    pub async fn list(&self, collection: Option<&str>) -> Result<Vec<KnowledgeDocument>> {
        let rows = match collection {
            Some(col) => {
                sqlx::query_as::<_, KnowledgeDocument>(
                    "SELECT * FROM knowledge_document WHERE collection = $1 \
                     ORDER BY updated_at DESC",
                )
                .bind(col)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query_as::<_, KnowledgeDocument>(
                    "SELECT * FROM knowledge_document ORDER BY updated_at DESC",
                )
                .fetch_all(&self.pool)
                .await?
            }
        };
        Ok(rows)
    }

    /// 白名单 collection 下的文档 id（去重排序，检索过滤用）。
    pub async fn ids_by_collections(&self, collections: &[String]) -> Result<Vec<Uuid>> {
        if collections.is_empty() {
            return Ok(Vec::new());
        }
        let mut ids: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM knowledge_document WHERE collection = ANY($1)")
                .bind(collections)
                .fetch_all(&self.pool)
                .await?;
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    /// 按 id 批量取文档（检索溯源用）。
    pub async fn by_ids(&self, ids: &[Uuid]) -> Result<Vec<KnowledgeDocument>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let rows = sqlx::query_as::<_, KnowledgeDocument>(
            "SELECT * FROM knowledge_document WHERE id = ANY($1)",
        )
        .bind(ids)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// 事务内整文档重建：删旧（级联删块）→ 插新文档 + 全部块（含向量）。
    ///
    /// 调用方负责在事务外生成 embeddings（网络 RPC 不持有连接）；此处只做 DB 写，
    /// 任一步失败整体回滚（无孤儿行，重跑安全）。
    pub async fn upsert_document(
        &self,
        source: &str,
        doc: &KnowledgeDocument,
        chunks: &[KnowledgeChunk],
        embeddings: &[Vec<f32>],
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // 级联删旧（块行即向量载体，随行删除）
        sqlx::query(
            "DELETE FROM knowledge_chunk \
             WHERE document_id IN (SELECT id FROM knowledge_document WHERE source = $1)",
        )
        .bind(source)
        .execute(&mut *tx)
        .await?;
        sqlx::query("DELETE FROM knowledge_document WHERE source = $1")
            .bind(source)
            .execute(&mut *tx)
            .await?;

        sqlx::query(
            "INSERT INTO knowledge_document \
             (id, title, collection, source, content, meta, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(doc.id)
        .bind(&doc.title)
        .bind(&doc.collection)
        .bind(&doc.source)
        .bind(&doc.content)
        .bind(doc.meta.as_ref())
        .bind(jiff_sqlx::Timestamp::from(doc.created_at))
        .bind(jiff_sqlx::Timestamp::from(doc.updated_at))
        .execute(&mut *tx)
        .await?;

        for (chunk, emb) in chunks.iter().zip(embeddings) {
            // 向量写入与块行同事务（行即向量载体）
            sqlx::query(
                "INSERT INTO knowledge_chunk \
                 (id, document_id, seq, content, created_at, embedding) \
                 VALUES ($1, $2, $3, $4, $5, $6::vector)",
            )
            .bind(chunk.id)
            .bind(chunk.document_id)
            .bind(chunk.seq)
            .bind(&chunk.content)
            .bind(jiff_sqlx::Timestamp::from(chunk.created_at))
            .bind(pgvector::vec_to_pgstring(emb))
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// 删除文档：级联删块（向量随行删除），事务内。
    pub async fn delete(&self, document_id: Uuid) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM knowledge_chunk WHERE document_id = $1")
            .bind(document_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM knowledge_document WHERE id = $1")
            .bind(document_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}
