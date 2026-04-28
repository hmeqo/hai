//! 话题组件 - 构建 RenderElement

use crate::{
    agent::context::render_context::{format_relative_time, format_time_dyn},
    agentcore::render::elements::{Item, item},
    domain::entity::Topic,
};

/// 构建单个话题元素（完整上下文，含时间信息）
/// `is_idle` 为 true 时添加 `idle` 属性标记
pub fn topic_element(topic: &Topic, is_idle: bool) -> Item {
    let started_at = format_relative_time(topic.started_at());
    let last_active = format_relative_time(topic.last_active_at());

    let mut el = item("topic");

    if is_idle {
        el = el.with_attr("idle", true);
    }

    el.with_attr("id", topic.id)
        .with_attr("started_at", started_at)
        .with_attr("last_active", last_active)
        .with_attr("title", topic.title.as_deref().unwrap_or("No Title"))
        .with_content(topic.summary.as_deref().unwrap_or("No Summary"))
}

/// 构建单个话题元素（无 RenderContext，用于 tool 响应等场景）
/// 使用独立的格式化函数，不依赖 ctx。
pub fn topic_element_static(topic: &Topic) -> Item {
    let started_at = format_time_dyn(topic.started_at());

    item("topic")
        .with_attr("id", topic.id)
        .with_attr("started_at", started_at)
        .with_attr("title", topic.title.as_deref().unwrap_or("No Title"))
        .with_content(topic.summary.as_deref().unwrap_or("No Summary"))
}
