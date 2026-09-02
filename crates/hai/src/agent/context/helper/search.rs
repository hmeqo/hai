use std::collections::HashSet;

use linkify::LinkFinder;
use uuid::Uuid;

use crate::{
    agent::context::types::{ParsedContent, SearchResult},
    agentcore::embedding::EmbeddingService,
    config::AppConfig,
    domain::{
        model::{Perception, Topic},
        service::DbServices,
        vo::ChatId,
    },
    error::Result,
};

/// 话题 title/summary + 消息文本 + 感知内容；记忆/话题/知识库检索共用同一 query。
pub fn build_search_query(
    topics: &[Topic],
    parsed: &[ParsedContent],
    perceptions: &[Perception],
) -> String {
    topics
        .iter()
        .flat_map(|t| [t.title.clone(), t.summary.clone()])
        .flatten()
        .chain(parsed.iter().map(|p| p.text.clone()))
        .chain(perceptions.iter().map(|p| p.content.clone()))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn search_related_context(params: SearchRelatedParams<'_>) -> Result<SearchResult> {
    let search_query = build_search_query(params.topics, params.parsed, params.perceptions);

    if search_query.is_empty() {
        return Ok(SearchResult {
            memories: Vec::new(),
            topics: Vec::new(),
        });
    }

    let embedding = params
        .services
        .multimodal
        .generate_embedding(&search_query)
        .await?;
    let ctx_cfg = &params.cfg.agent.context;
    let (memories, mut related_topics) = tokio::try_join!(
        params.services.memory.search_related(
            params.chat_id,
            &embedding,
            ctx_cfg.related_memory_limit
        ),
        params.services.topic.search_related_topics(
            params.chat_id,
            &embedding,
            ctx_cfg.related_topic_limit
        ),
    )?;

    let active_ids: HashSet<Uuid> = params.topics.iter().map(|t| t.id).collect();
    related_topics.retain(|r| !active_ids.contains(&r.topic.id));
    Ok(SearchResult {
        memories,
        topics: related_topics,
    })
}

pub(crate) struct SearchRelatedParams<'a> {
    pub services: &'a DbServices,
    pub cfg: &'a AppConfig,
    pub chat_id: ChatId,
    pub topics: &'a [Topic],
    pub parsed: &'a [ParsedContent],
    pub perceptions: &'a [Perception],
    pub shown_memory_ids: &'a HashSet<Uuid>,
    pub shown_topic_ids: &'a HashSet<Uuid>,
}

/// 后续轮自动按 2/3 缩减（5→3, 3→2）。
pub async fn search_related_dedup(params: SearchRelatedParams<'_>) -> Result<SearchResult> {
    let SearchResult { memories, topics } = search_related_context(SearchRelatedParams {
        shown_memory_ids: params.shown_memory_ids,
        shown_topic_ids: params.shown_topic_ids,
        services: params.services,
        cfg: params.cfg,
        chat_id: params.chat_id,
        topics: params.topics,
        parsed: params.parsed,
        perceptions: params.perceptions,
    })
    .await?;

    if params.shown_memory_ids.is_empty() && params.shown_topic_ids.is_empty() {
        return Ok(SearchResult { memories, topics });
    }

    let ctx_cfg = &params.cfg.agent.context;
    let ml = (ctx_cfg.related_memory_limit * 2 / 3) as usize;
    let tl = (ctx_cfg.related_topic_limit * 2 / 3) as usize;

    Ok(SearchResult {
        memories: memories
            .into_iter()
            .filter(|m| !params.shown_memory_ids.contains(&m.id.0))
            .take(ml)
            .collect(),
        topics: topics
            .into_iter()
            .filter(|t| !params.shown_topic_ids.contains(&t.topic.id))
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
