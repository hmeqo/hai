//! 聊天渲染组件

use crate::{
    agentcore::render::elements::{Section, section},
    domain::entity::Chat,
};

/// 渲染聊天信息
pub fn render_chat_info(chat: &Chat) -> Section {
    let mut builder = section("chat")
        .with_attr("id", chat.id)
        .with_attr("platform", chat.platform.as_str())
        .with_attr("type", chat.chat_type.as_str())
        .with_attr("created_at", chat.created_at());

    if let Some(name) = &chat.name {
        builder = builder.with_attr("name", name.as_str());
    }

    builder
}
