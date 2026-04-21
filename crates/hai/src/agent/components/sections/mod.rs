//! 组件模块
//!
//! 提供从业务数据构建 RenderElement 的组件

pub mod account;
pub mod chat;
pub mod context;
pub mod memory;
pub mod message;
pub mod topic;

pub use account::{account_element, accounts_elements, accounts_section};
pub use context::{build_context_section, render_main_context};
pub use memory::{memories_elements, memories_section, related_memories_elements};
pub use message::{
    conversation_element, conversation_section, message_element, messages_elements,
    messages_section,
};
pub use topic::{
    topic_element, topic_search_element, topic_search_elements, topic_search_section,
    topics_elements, topics_section,
};

use crate::agent::render::elements::Section;
use crate::domain::entity::{Account, Topic};

/// 便捷方法: 构建 accounts section
pub fn involved_accounts_section(accounts: &[Account]) -> Section {
    account::accounts_section(accounts, "accounts")
}

/// 便捷方法: 构建 memories section
pub fn memory_section(memories: &[crate::domain::entity::Memory]) -> Section {
    memory::memories_section(memories, "memories")
}

/// 便捷方法: 构建 topics section（无上下文版本，仅供工具等简单场景使用）
pub fn topic_section(topics: &[Topic]) -> Section {
    topic::topics_section(
        topics,
        &crate::agent::components::RenderContext::new(&[], &[], &[], 0),
        "topics",
    )
}

/// 便捷方法: 构建 related_topics section
pub fn related_topics_section(topics: &[crate::domain::vo::TopicSearchResult]) -> Section {
    topic::topic_search_section(topics, "related_topics")
}

/// 便捷方法: 构建 related_memories section
pub fn related_memories_section(
    memories: &[crate::domain::service::memory::RelatedMemory],
    tag: &str,
) -> Section {
    memory::related_memories_section(memories, tag)
}

/// 便捷方法: 构建 chat info section
pub fn chat_info(chat: &crate::domain::entity::Chat) -> Section {
    chat::render_chat_info(chat)
}
