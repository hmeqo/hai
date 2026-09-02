//! 知识库服务：文档生命周期 + 分块 + 嵌入 + 检索。

use std::{collections::HashMap, sync::Arc};

use futures::{TryStreamExt, stream::StreamExt};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    agentcore::embedding::EmbeddingService,
    domain::{
        model::{KnowledgeChunk, KnowledgeDocument, KnowledgeDocumentMeta},
        repo::Repos,
        vo::{KnowledgeChunkId, KnowledgeDocumentId},
    },
    error::{ErrorKind, Result},
    util::chunking::{self, ChunkCfg},
};

/// 当前分块算法版本。算法/参数变更时升版本，`reindex` 据此重建存量块。
pub const CHUNKER_VERSION: &str = "v1";

/// 单文档块数上限：超过拒绝导入（不静默截断，知识库完整性优先）。
pub const MAX_CHUNKS_PER_DOC: usize = 10_000;

/// 批量嵌入并发度（对齐 rebuild.rs 的 Semaphore 范式）。
const MAX_CONCURRENT_EMBEDDINGS: usize = 10;

#[derive(Debug)]
pub struct KnowledgeService {
    repos: Repos,
    embedding: Arc<dyn EmbeddingService>,
}

/// 检索命中：块 + 文档溯源。
#[derive(Debug)]
pub struct RelatedChunk {
    pub chunk_id: KnowledgeChunkId,
    pub document_id: KnowledgeDocumentId,
    pub content: String,
    pub document_title: String,
    pub collection: String,
    pub distance: f64,
}

/// 导入结果。
#[derive(Debug)]
pub struct ImportOutcome {
    /// false = 内容未变（同 source + 同 hash），跳过
    pub imported: bool,
    pub chunk_count: usize,
}

impl KnowledgeService {
    pub fn new(repos: Repos, embedding: Arc<dyn EmbeddingService>) -> Self {
        Self { repos, embedding }
    }

    /// 导入/更新文档（幂等）：同 source + 同内容哈希 → 跳过；否则整文档替换重建。
    ///
    /// `force = true` 跳过哈希幂等检查（reindex 用：chunker 版本变更时即使内容未变
    /// 也要重切重建；也用于更新 meta.chunker_version）。
    ///
    /// 嵌入在事务外批量生成（网络 RPC 不持有连接）；事务内只做 DB 写（建文档/块 +
    /// 向量），失败整体回滚无孤儿，重跑安全。
    pub async fn upsert_document(
        &self,
        source: &str,
        title: &str,
        collection: &str,
        content: &str,
        cfg: &ChunkCfg,
        force: bool,
    ) -> Result<ImportOutcome> {
        let file_hash = sha256_hex(content);

        let existing = self.repos.knowledge_document.find_by_source(source).await?;

        if !force
            && let Some(doc) = &existing
            && parse_doc_meta(doc).is_some_and(|m| m.file_hash == file_hash)
        {
            return Ok(ImportOutcome {
                imported: false,
                chunk_count: 0,
            });
        }

        let chunks = chunking::chunk(content, cfg);
        if chunks.len() > MAX_CHUNKS_PER_DOC {
            return Err(ErrorKind::ValidationFailed.msg(format!(
                "document '{source}' yields {} chunks, exceeding limit {MAX_CHUNKS_PER_DOC}",
                chunks.len()
            )));
        }

        // 事务外批量嵌入（网络 RPC 不持有连接；失败上抛——尚无任何写入，无回滚需求）。
        let embeddings: Vec<Vec<f32>> = futures::stream::iter(chunks.iter().cloned())
            .map(|chunk_text| async move { self.embedding.generate_embedding(&chunk_text).await })
            .buffered(MAX_CONCURRENT_EMBEDDINGS)
            .try_collect()
            .await?;

        // 文档 upsert + 级联删块 + 批量插块（含向量）在同一事务内（repo 封装）
        let meta_json = serde_json::to_value(KnowledgeDocumentMeta {
            file_hash,
            chunker_version: CHUNKER_VERSION.to_string(),
        })?;
        let doc = KnowledgeDocument::new(
            title.to_string(),
            collection.to_string(),
            source.to_string(),
            content.to_string(),
            Some(meta_json),
        );
        let chunk_rows: Vec<KnowledgeChunk> = chunks
            .iter()
            .enumerate()
            .map(|(seq, chunk_text)| {
                KnowledgeChunk::new(KnowledgeDocumentId(doc.id), seq as i32, chunk_text.clone())
            })
            .collect();
        self.repos
            .knowledge_document
            .upsert_document(source, &doc, &chunk_rows, &embeddings)
            .await?;

        Ok(ImportOutcome {
            imported: true,
            chunk_count: chunks.len(),
        })
    }

    /// 删除文档：级联删块（向量随行删除）。
    pub async fn delete(&self, document_id: KnowledgeDocumentId) -> Result<()> {
        self.repos.knowledge_document.delete(document_id.0).await
    }

    /// 文档列表（可按 collection 过滤，更新时间倒序）。
    pub async fn list(&self, collection: Option<&str>) -> Result<Vec<KnowledgeDocument>> {
        self.repos.knowledge_document.list(collection).await
    }

    /// 语义检索：query 嵌入 → knowledge_chunk 向量检索 → 文档溯源。
    /// `collections` 非空 = collection 白名单（先取白名单文档 id，再以参数化
    /// `= ANY($1)` 过滤向量检索）；空 = 全部库。
    pub async fn search(
        &self,
        query: &str,
        limit: i64,
        collections: &[String],
    ) -> Result<Vec<RelatedChunk>> {
        // 边界防护：负数 LIMIT 在 PG 中语义未定义，超大值会触发 IN 列表参数上限
        let limit = limit.clamp(1, 1000);
        let query_vec = self.embedding.generate_embedding(query).await?;

        let filter_ids: Option<Vec<Uuid>> = if collections.is_empty() {
            None
        } else {
            let ids = self
                .repos
                .knowledge_document
                .ids_by_collections(collections)
                .await?;
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            Some(ids)
        };

        let rows = self
            .repos
            .knowledge_chunk
            .search_by_embedding(&query_vec, filter_ids.as_deref(), limit)
            .await?;
        if rows.is_empty() {
            return Ok(Vec::new());
        }

        let chunk_ids: Vec<Uuid> = rows.iter().map(|(id, _)| *id).collect();
        let chunks = self.repos.knowledge_chunk.by_ids(&chunk_ids).await?;
        let doc_ids: Vec<Uuid> = chunks.iter().map(|c| c.document_id).collect();
        let docs = self.repos.knowledge_document.by_ids(&doc_ids).await?;

        let chunk_map: HashMap<Uuid, KnowledgeChunk> =
            chunks.into_iter().map(|c| (c.id, c)).collect();
        let doc_map: HashMap<Uuid, KnowledgeDocument> =
            docs.into_iter().map(|d| (d.id, d)).collect();

        Ok(rows
            .into_iter()
            .filter_map(|(id, dist)| {
                let chunk = chunk_map.get(&id)?;
                let doc = doc_map.get(&chunk.document_id)?;
                Some(RelatedChunk {
                    chunk_id: KnowledgeChunkId(id),
                    document_id: KnowledgeDocumentId(doc.id),
                    content: chunk.content.clone(),
                    document_title: doc.title.clone(),
                    collection: doc.collection.clone(),
                    distance: dist,
                })
            })
            .collect())
    }

    /// 重新分块 + 重新嵌入：`meta.chunker_version != CHUNKER_VERSION` 的文档重建。
    /// 返回处理的文档数。与 `db rebuild embeddings`（只重算向量）职责分离。
    pub async fn reindex(&self, collection: Option<&str>, cfg: &ChunkCfg) -> Result<usize> {
        let docs = self.repos.knowledge_document.list(collection).await?;

        let mut rebuilt = 0usize;
        for doc in docs {
            if parse_doc_meta(&doc).is_some_and(|m| m.chunker_version == CHUNKER_VERSION) {
                continue; // 版本一致，块有效
            }
            // 从存储的原文精确重切（chunk 是派生数据，原文为准）；force 跳过哈希
            // 幂等检查——版本过期文档内容未变也要重建，并更新 chunker_version
            self.upsert_document(
                &doc.source,
                &doc.title,
                &doc.collection,
                &doc.content,
                cfg,
                true,
            )
            .await?;
            rebuilt += 1;
        }
        Ok(rebuilt)
    }
}

/// 解析文档 meta；解析失败（meta 损坏是异常数据）warn 并视为无 meta
/// （无 meta 会触发重建而非静默保留坏数据）。
fn parse_doc_meta(doc: &KnowledgeDocument) -> Option<KnowledgeDocumentMeta> {
    let json = doc.meta.as_ref()?.clone();
    match serde_json::from_value(json) {
        Ok(m) => Some(m),
        Err(e) => {
            tracing::warn!(doc_id = %doc.id, error = %e, "knowledge document meta is corrupted");
            None
        }
    }
}

/// sha256 内容哈希（hex）。
fn sha256_hex(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}
