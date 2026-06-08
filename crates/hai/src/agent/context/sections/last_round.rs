use crate::{agent::runtime::round::Round, agentcore::render::elements::Node};

/// 构建 `<last-round>` 节点（工具调用结果 + 内部笔记）
pub fn build_last_round_section(prev_round: &Round) -> Option<Node> {
    let mut toolcall_nodes: Vec<Node> = Vec::new();
    for t in &prev_round.tool_calls {
        toolcall_nodes.push(
            Node::tag("toolcall")
                .attr("name", &t.tool_name)
                .attr("query", t.arguments.to_string())
                .child(Node::text(t.result.to_string())),
        );
    }

    let mut children: Vec<Node> = Vec::new();

    if !toolcall_nodes.is_empty() {
        children.push(Node::tag("toolcalls").children(toolcall_nodes));
    }

    if let Some(resp) = &prev_round.notes {
        children.push(Node::tag("internal").child(Node::tag("notes").child(Node::text(resp))));
    }

    if children.is_empty() {
        None
    } else {
        Some(Node::tag("last-round").children(children))
    }
}
