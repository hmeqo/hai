//! 记忆组件 - 构建 XML 节点

use crate::{
    agent::context::fmt::format_time_dyn, agentcore::render::elements::Node,
    domain::service::memory::RelatedMemory,
};

pub(super) fn related_memory_element(mem: &RelatedMemory) -> Node {
    let source = mem
        .account_id
        .map(|id| format!("UserID:{}", id))
        .unwrap_or_else(|| "System".into());

    Node::tag("memory")
        .attr("id", mem.id.0)
        .attr("source", source)
        .attr("relevance", format!("{:.4}", mem.distance))
        .attr("created_at", format_time_dyn(mem.created_at))
        .child(Node::text(&mem.content))
}

pub(super) fn related_memories_elements(memories: &[RelatedMemory]) -> Vec<Node> {
    memories.iter().map(related_memory_element).collect()
}

/// 构建相关记忆 Section
pub fn related_memories_section(memories: &[RelatedMemory], tag: &str) -> Node {
    Node::tag(tag).children(related_memories_elements(memories))
}
