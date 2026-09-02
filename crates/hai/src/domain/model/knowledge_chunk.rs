use uuid::Uuid;

use crate::domain::vo::{KnowledgeChunkId, KnowledgeDocumentId};

/// 知识库块：检索单位（每块独立嵌入）。
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KnowledgeChunk {
    pub id: uuid::Uuid,
    /// 所属文档（UNIQUE(document_id, seq)：块序列稳定，重复导入可比对）
    pub document_id: uuid::Uuid,
    /// 文档内顺序（0..n）
    pub seq: i32,
    /// 块文本（≤ chunk_max 字符，含标题路径前缀）
    pub content: String,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub created_at: jiff::Timestamp,
}

impl KnowledgeChunk {
    pub fn id_(&self) -> KnowledgeChunkId {
        KnowledgeChunkId(self.id)
    }

    pub fn new(document_id: KnowledgeDocumentId, seq: i32, content: String) -> Self {
        Self {
            id: Uuid::now_v7(),
            document_id: document_id.into(),
            seq,
            content,
            created_at: jiff::Timestamp::now(),
        }
    }
}
