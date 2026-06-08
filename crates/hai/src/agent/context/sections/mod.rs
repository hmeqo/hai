pub mod account;
pub mod chat;
pub mod context;
pub mod fmt;
pub mod last_round;
pub mod memory;
pub mod message;
pub mod perception;
pub mod topic;

pub use account::account_element;
pub use context::{build_context_section, build_situation_section, render_main_context};
pub use last_round::build_last_round_section;
pub use message::conversation_element;
pub use topic::{topic_element, topic_element_static};

use crate::{
    agentcore::render::elements::Node,
    domain::{entity::Topic, service::memory::RelatedMemory},
};

pub fn topic_section(topics: &[Topic]) -> Node {
    Node::tag("topics").children(topics.iter().map(topic_element_static).collect::<Vec<_>>())
}

pub fn related_memories_section(memories: &[RelatedMemory], tag: &str) -> Node {
    memory::related_memories_section(memories, tag)
}
