//! 记忆组件 - 构建 RenderElement

use crate::{
    agent::context::render_context::format_time_dyn,
    agentcore::render::elements::{RenderElement, Section, item, section},
    domain::{entity::Memory, service::memory::RelatedMemory},
};

/// 构建单个记忆元素
pub fn memory_element(memory: &Memory) -> RenderElement {
    item("memory")
        .with_attr("id", memory.id)
        .with_attr("type", memory.type_.as_str())
        .with_attr("created_at", format_time_dyn(memory.created_at()))
        .with_content(&memory.content)
        .into_element()
}

/// 构建记忆列表元素
pub fn memories_elements(memories: &[Memory]) -> Vec<RenderElement> {
    memories.iter().map(memory_element).collect()
}

/// 构建记忆 Section
pub fn memories_section(memories: &[Memory], tag: &str) -> Section {
    section(tag).add_children(memories_elements(memories))
}

/// 构建相关记忆元素（带 relevance）
pub fn related_memory_element(mem: &RelatedMemory) -> RenderElement {
    let source = mem
        .account_id
        .map(|id| format!("UserID:{}", id))
        .unwrap_or_else(|| "System".into());

    item("memory")
        .with_attr("id", mem.id)
        .with_attr("source", source)
        .with_attr("relevance", format!("{:.4}", mem.distance))
        .with_attr("created_at", format_time_dyn(mem.created_at))
        .with_content(&mem.content)
        .into_element()
}

/// 构建相关记忆列表元素
pub fn related_memories_elements(memories: &[RelatedMemory]) -> Vec<RenderElement> {
    memories.iter().map(related_memory_element).collect()
}

/// 构建相关记忆 Section
pub fn related_memories_section(memories: &[RelatedMemory], tag: &str) -> Section {
    section(tag).add_children(related_memories_elements(memories))
}
