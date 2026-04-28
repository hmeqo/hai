//! 渲染组件模块
//!
//! 从业务数据构建 RenderElement 的纯函数集合。

pub mod account;
pub mod chat;
pub mod context;
pub mod memory;
pub mod message;
pub mod topic;

pub use account::{account_element, accounts_elements, accounts_section};
pub use context::{build_context_section, render_main_context};
pub use memory::{memories_elements, memories_section, related_memories_elements};
pub use message::{conversation_element, message_element, messages_elements, messages_section};
pub use topic::{topic_element, topic_element_static};

use crate::{
    agentcore::render::{elements::Section, section},
    domain::entity::{Account, Topic},
};

// ── 无 ctx 的便捷构建函数（供 tool 等无完整上下文的场景使用）─────────────────

pub fn involved_accounts_section(accounts: &[Account]) -> Section {
    account::accounts_section(accounts, "accounts")
}

pub fn memory_section(memories: &[crate::domain::entity::Memory]) -> Section {
    memory::memories_section(memories, "memories")
}

/// 话题 Section
pub fn topic_section(topics: &[Topic]) -> Section {
    section("topics").add_children(topics.iter().map(topic_element_static))
}

pub fn related_memories_section(
    memories: &[crate::domain::service::memory::RelatedMemory],
    tag: &str,
) -> Section {
    memory::related_memories_section(memories, tag)
}

pub fn chat_info(chat: &crate::domain::entity::Chat) -> Section {
    chat::render_chat_info(chat)
}
