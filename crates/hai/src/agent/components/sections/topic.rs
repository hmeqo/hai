//! 话题组件 - 构建 RenderElement

use jiff::tz::TimeZone;

use crate::agent::components::context::RenderContext;
use crate::agent::render::elements::{RenderElement, Section, item, section};
use crate::domain::entity::Topic;
use crate::domain::vo::TopicSearchResult;

/// 构建单个话题元素
pub fn topic_element(topic: &Topic, ctx: &RenderContext) -> RenderElement {
    let started_at = format!(
        "{} ({})",
        topic.started_at().to_zoned(TimeZone::system()),
        ctx.format_relative_time(topic.started_at())
    );
    let last_active = ctx.format_relative_time(topic.last_active_at());

    item("topic")
        .with_attr("id", topic.id)
        .with_attr("started_at", started_at)
        .with_attr("last_active", last_active)
        .with_attr("title", topic.title.as_deref().unwrap_or("No Title"))
        .with_content(topic.summary.as_deref().unwrap_or("No Summary"))
        .into_element()
}

/// 构建话题列表元素
pub fn topics_elements(topics: &[Topic], ctx: &RenderContext) -> Vec<RenderElement> {
    topics.iter().map(|t| topic_element(t, ctx)).collect()
}

/// 构建话题 Section
pub fn topics_section(topics: &[Topic], ctx: &RenderContext, tag: &str) -> Section {
    section(tag).add_children(topics_elements(topics, ctx))
}

/// 构建搜索结果话题元素
pub fn topic_search_element(result: &TopicSearchResult) -> RenderElement {
    item("topic")
        .with_attr("id", result.topic.id)
        .with_attr("relevance", format!("{:.4}", result.distance))
        .with_attr("title", result.topic.title.as_deref().unwrap_or("No Title"))
        .with_content(result.topic.summary.as_deref().unwrap_or("No Summary"))
        .into_element()
}

/// 构建话题搜索结果列表元素
pub fn topic_search_elements(results: &[TopicSearchResult]) -> Vec<RenderElement> {
    results.iter().map(topic_search_element).collect()
}

/// 构建话题搜索结果 Section
pub fn topic_search_section(results: &[TopicSearchResult], tag: &str) -> Section {
    section(tag).add_children(topic_search_elements(results))
}
