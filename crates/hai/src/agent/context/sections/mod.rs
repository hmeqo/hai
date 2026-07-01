pub mod account;
pub mod chat;
pub mod context;
pub mod fmt;
pub mod memory;
pub mod message;
pub mod perception;
pub mod topic;

use crate::{
    agentcore::render::elements::Node,
    domain::{model::Topic, service::memory::RelatedMemory, vo::TopicSearchResult},
};

pub fn topic_section(topics: &[Topic]) -> Node {
    Node::tag("topics").children(
        topics
            .iter()
            .map(topic::topic_element_static)
            .collect::<Vec<_>>(),
    )
}

pub fn related_memories_section(memories: &[RelatedMemory], tag: &str) -> Node {
    memory::related_memories_section(memories, tag)
}

pub fn related_topics_section(topics: &[TopicSearchResult]) -> Node {
    Node::tag("related_topics").children(
        topics
            .iter()
            .map(|r| {
                topic::topic_element_static(&r.topic)
                    .attr("relevance", format!("{:.4}", r.distance))
            })
            .collect::<Vec<_>>(),
    )
}
