use crate::{agent::runtime::round::Round, agentcore::render::elements::Node};

/// 构建 `<round-end>` 节点（工具调用结果）
pub fn build_round_end_section(prev_round: &Round) -> Option<Node> {
    let mut children: Vec<Node> = Vec::new();

    let toolcall_nodes: Vec<Node> = prev_round
        .tool_calls
        .iter()
        .map(|t| {
            Node::tag("toolcall")
                .attr("name", &t.tool_name)
                .attr("query", t.arguments.to_string())
                .child(Node::text(t.result.to_string()))
        })
        .collect();

    if !toolcall_nodes.is_empty() {
        children.push(Node::tag("toolcalls").children(toolcall_nodes));
    }

    if children.is_empty() {
        None
    } else {
        Some(Node::tag("round-end").children(children))
    }
}
