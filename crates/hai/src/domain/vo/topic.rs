use crate::domain::model::Topic;

/// 带相似度距离的话题检索结果（pgvector `<->` L2 距离，越小越相似）
#[derive(Debug, Clone)]
pub struct TopicSearchResult {
    pub topic: Topic,
    pub distance: f64,
}
