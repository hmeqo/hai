use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::vo::KnowledgeDocumentId;

/// 知识库文档：生命周期单位（导入/更新/删除粒度）。
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct KnowledgeDocument {
    pub id: uuid::Uuid,

    /// 文档标题（frontmatter title 或文件名）
    pub title: String,
    /// 分库标签；空字符串 = 未分类
    pub collection: String,
    /// 来源（导入时路径原文或 "text"）
    pub source: String,
    /// 文档原文（reindex 精确重切的依据；块是派生数据）
    pub content: String,
    /// 扩展元数据：KnowledgeDocumentMeta（file_hash / chunker_version）
    pub meta: Option<serde_json::Value>,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub created_at: jiff::Timestamp,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub updated_at: jiff::Timestamp,
}

impl KnowledgeDocument {
    pub fn id_(&self) -> KnowledgeDocumentId {
        KnowledgeDocumentId(self.id)
    }

    pub fn new(
        title: String,
        collection: String,
        source: String,
        content: String,
        meta: Option<serde_json::Value>,
    ) -> Self {
        Self {
            id: Uuid::now_v7(),
            title,
            collection,
            source,
            content,
            meta,
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        }
    }
}

/// 文档元数据（meta 列的 typed 内容，无魔法字符串）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeDocumentMeta {
    /// 内容哈希（sha256），导入幂等比对依据
    pub file_hash: String,
    /// 分块算法版本（未来参数/算法变更时 reindex 比对）
    pub chunker_version: String,
}
