//! 知识库组件 - 构建 XML 节点

use crate::{agentcore::render::elements::Node, domain::service::knowledge::RelatedChunk};

fn knowledge_item(chunk: &RelatedChunk) -> Node {
    let collection = if chunk.collection.is_empty() {
        "-"
    } else {
        chunk.collection.as_str()
    };
    Node::tag("knowledge_item")
        .attr("id", chunk.chunk_id.0)
        .attr("document", &chunk.document_title)
        .attr("collection", collection)
        .attr("relevance", format!("{:.4}", chunk.distance))
        .child(Node::text(&chunk.content))
}

/// RAG 自动检索注入；与 related_memories 同构。
pub fn related_knowledge_section(chunks: &[RelatedChunk]) -> Node {
    Node::tag("knowledge").children(chunks.iter().map(knowledge_item).collect::<Vec<_>>())
}
