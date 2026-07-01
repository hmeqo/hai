use std::collections::HashSet;

use linkify::LinkFinder;
use uuid::Uuid;

use crate::{
    agent::context::types::{ParsedContent, SearchResult},
    config::AppConfig,
    domain::{
        model::{Perception, Topic},
        service::DbServices,
        vo::ChatId,
    },
    error::Result,
};

/// 向量搜索相关内容（记忆+话题）
pub async fn search_related_context(
    services: &DbServices,
    cfg: &AppConfig,
    chat_id: ChatId,
    topics: &[Topic],
    parsed: &[ParsedContent],
    perceptions: &[Perception],
) -> Result<SearchResult> {
    let search_query: String = topics
        .iter()
        .flat_map(|t| [t.title.clone(), t.summary.clone()])
        .flatten()
        .chain(parsed.iter().map(|p| p.text.clone()))
        .chain(perceptions.iter().map(|p| p.content.clone()))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if search_query.is_empty() {
        return Ok(SearchResult {
            memories: Vec::new(),
            topics: Vec::new(),
        });
    }

    let embedding = services
        .multimodal
        .generate_embedding(&search_query)
        .await?;
    let ctx_cfg = &cfg.agent.context;
    let (memories, mut related_topics) = match tokio::try_join!(
        services
            .memory
            .search_related(chat_id, &embedding, ctx_cfg.related_memory_limit),
        services
            .topic
            .search_related_topics(chat_id, &embedding, ctx_cfg.related_topic_limit),
    ) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(%chat_id, "Vector search failed (try clearing old embeddings?): {e}");
            (Vec::new(), Vec::new())
        }
    };

    let active_ids: HashSet<Uuid> = topics.iter().map(|t| t.id).collect();
    related_topics.retain(|r| !active_ids.contains(&r.topic.id));
    Ok(SearchResult {
        memories,
        topics: related_topics,
    })
}

/// 检索相关内容，排除已展示项。后续轮自动按 2/3 缩减（5→3, 3→2）。
pub async fn search_related_dedup(
    services: &DbServices,
    cfg: &AppConfig,
    chat_id: ChatId,
    topics: &[Topic],
    parsed: &[ParsedContent],
    perceptions: &[Perception],
    shown_memory_ids: &HashSet<Uuid>,
    shown_topic_ids: &HashSet<Uuid>,
) -> Result<SearchResult> {
    let SearchResult { memories, topics } =
        search_related_context(services, cfg, chat_id, topics, parsed, perceptions).await?;

    if shown_memory_ids.is_empty() && shown_topic_ids.is_empty() {
        return Ok(SearchResult { memories, topics });
    }

    let cfg = &cfg.agent.context;
    let ml = (cfg.related_memory_limit * 2 / 3) as usize;
    let tl = (cfg.related_topic_limit * 2 / 3) as usize;

    Ok(SearchResult {
        memories: memories
            .into_iter()
            .filter(|m| !shown_memory_ids.contains(&m.id.0))
            .take(ml)
            .collect(),
        topics: topics
            .into_iter()
            .filter(|t| !shown_topic_ids.contains(&t.topic.id))
            .take(tl)
            .collect(),
    })
}

pub fn extract_urls(text: &str) -> Vec<String> {
    let mut finder = LinkFinder::new();
    finder
        .kinds(&[linkify::LinkKind::Url])
        .links(text)
        .map(|l| l.as_str().to_string())
        .collect()
}
