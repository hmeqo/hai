use std::collections::HashMap;

use uuid::Uuid;

use crate::{
    agent::context::render_context::ContentRenderer,
    domain::{entity::Perception, service::memory::RelatedMemory, vo::TopicSearchResult},
};

/// 附件感知查找表
pub struct AttachmentPerceptionMap {
    pub by_attachment_id: HashMap<Uuid, Vec<Perception>>,
    pub same_resource_as: HashMap<Uuid, Uuid>,
}

/// 消息中的附件
pub struct Attachment {
    pub id: Uuid,
    pub file_id: String,
}

/// 一次解析的结果
pub struct ParsedContent {
    pub text: String,
    pub attachments: Vec<Attachment>,
    pub text_fragments: Vec<String>,
}

/// 感知查询结果
pub struct PerceptionResult {
    pub items: Vec<Perception>,
    pub map: AttachmentPerceptionMap,
}

/// 相关上下文搜索结果
pub struct SearchResult {
    pub memories: Vec<RelatedMemory>,
    pub topics: Vec<TopicSearchResult>,
}

/// 平台内容解析器
pub trait ContentParser: Send + Sync {
    fn parse(&self, value: &serde_json::Value) -> ParsedContent;
    fn create_renderer(&self, map: &AttachmentPerceptionMap) -> ContentRenderer;
}
