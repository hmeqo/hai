use crate::{agentcore::render::elements::Node, domain::model::ChatRoster};

pub fn render_chat_info(chat: &ChatRoster) -> Node {
    render_chat_tag(chat, "chat")
}

pub fn render_current_chat(chat: &ChatRoster) -> Node {
    render_chat_tag(chat, "current_chat")
}

fn render_chat_tag(chat: &ChatRoster, tag: &str) -> Node {
    let mut b = Node::tag(tag)
        .attr("alias", chat.alias.as_str())
        .attr("id", chat.id.0)
        .attr("platform", chat.platform.as_str())
        .attr("type", chat.chat_type.as_str());

    if let Some(name) = &chat.name {
        b = b.attr("name", name.as_str());
    }

    b
}
