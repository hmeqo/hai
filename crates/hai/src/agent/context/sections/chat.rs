//! 聊天渲染组件

use crate::{agentcore::render::elements::Node, domain::model::Chat};

pub fn render_chat_info(chat: &Chat) -> Node {
    let mut b = Node::tag("chat")
        .attr("id", chat.id)
        .attr("platform", chat.platform.as_str())
        .attr("type", chat.chat_type.as_str())
        .attr("created_at", chat.created_at);

    if let Some(name) = &chat.name {
        b = b.attr("name", name.as_str());
    }

    b
}
