//! 话题组件 - 构建 XML 节点

use crate::{
    agent::context::fmt::{format_relative_time, format_time_dyn},
    agentcore::render::elements::Node,
    domain::model::Topic,
};

/// 构建单个话题元素（完整上下文，含时间信息）
/// `need_close` 为 true 时添加 `need-close` 属性
pub fn topic_element(topic: &Topic, need_close: bool) -> Node {
    let started_at = format_relative_time(topic.started_at);
    let last_active = format_relative_time(topic.last_active_at);

    let mut el = Node::tag("topic");

    if need_close {
        el = el.attr("need-close", true);
    }

    el.attr("id", topic.id)
        .attr("started_at", started_at)
        .attr("last_active", last_active)
        .attr("title", topic.title.as_deref().unwrap_or("No Title"))
        .child(Node::text(topic.summary.as_deref().unwrap_or("No Summary")))
}

/// 构建单个话题元素（无 RenderContext，用于 tool 响应等场景）
/// 使用独立的格式化函数，不依赖 ctx。
pub fn topic_element_static(topic: &Topic) -> Node {
    let started_at = format_time_dyn(topic.started_at);

    Node::tag("topic")
        .attr("id", topic.id)
        .attr("started_at", started_at)
        .attr("title", topic.title.as_deref().unwrap_or("No Title"))
        .child(Node::text(topic.summary.as_deref().unwrap_or("No Summary")))
}
