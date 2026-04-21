//! 消息组件 - 构建 RenderElement

use crate::agent::components::context::RenderContext;
use crate::agent::render::content;
use crate::agent::render::elements::{RenderElement, Section, item, section};
use crate::domain::entity::{Account, Message, MessageStatus, Topic};

/// 构建单条消息元素
pub fn message_element(msg: &Message, ctx: &RenderContext) -> RenderElement {
    let sender = ctx.get_sender_name(msg);
    let sent_at = ctx.format_sent_at(msg.sent_at());
    let topic_hint = ctx.get_topic_hint(msg);

    let is_replied = msg.status() == MessageStatus::Replied;

    let mut builder = item("message")
        .with_attr("id", msg.id)
        .with_attr("role", &msg.role)
        .with_attr("sender", sender.as_ref())
        .with_attr("sent_at", sent_at.as_ref())
        .with_attr("replied", is_replied);

    if !topic_hint.is_empty() {
        builder = builder.with_attr("topic", topic_hint.trim());
    }

    if let Some(reply_id) = msg.reply_to_id {
        let mut reply = item("reply_to").with_attr("id", reply_id);
        if let Some(replied_msg) = ctx.get_message(reply_id) {
            let full = content::render_content_from_value(&replied_msg.content);
            let peek = peek_text(&full, 10);
            if !peek.is_empty() {
                reply = reply.with_content(peek);
            }
        }
        builder = builder.add_child(reply);
    }

    let content_str = content::render_content_from_value(&msg.content);
    builder.with_content(content_str).into_element()
}

/// 构建消息列表元素
pub fn messages_elements(messages: &[&Message], ctx: &RenderContext) -> Vec<RenderElement> {
    messages.iter().map(|m| message_element(m, ctx)).collect()
}

/// 构建消息 Section
pub fn messages_section(messages: &[&Message], ctx: &RenderContext, tag: &str) -> Section {
    section(tag).add_children(messages_elements(messages, ctx))
}

/// 构建对话历史元素
pub fn conversation_element(messages: &[&Message], ctx: &RenderContext) -> RenderElement {
    if messages.is_empty() {
        return RenderElement::Empty;
    }

    let main_ids: std::collections::HashSet<i64> = messages.iter().map(|m| m.id).collect();

    let mut seen_reply_ids = std::collections::HashSet::<i64>::new();
    let reply_context_msgs: Vec<&Message> = messages
        .iter()
        .filter_map(|m| m.reply_to_id)
        .filter(|rid| !main_ids.contains(rid) && seen_reply_ids.insert(*rid))
        .filter_map(|rid| ctx.get_message(rid))
        .collect();

    let (history, unread): (Vec<_>, Vec<_>) = messages
        .iter()
        .partition(|m| m.interaction_status != "pending");

    let mut root = section("conversation");

    if !reply_context_msgs.is_empty() {
        let reply_section = section("reply_context")
            .add_children(messages_elements(reply_context_msgs.as_slice(), ctx));
        root.push_child(reply_section);
    }

    root.push_child(section("history").add_children(messages_elements(&history, ctx)));

    if !unread.is_empty() {
        root.push_child(section("unread").add_children(messages_elements(&unread, ctx)));
    }

    root.into_element()
}

/// 从渲染后的文本中截取前 `max_chars` 个字符作为预览
///
/// 按 Unicode 字符边界截断，超出时追加 `…`。
/// 过滤掉换行符，保持单行展示。
fn peek_text(text: &str, max_chars: usize) -> String {
    let single_line: String = text.chars().filter(|c| *c != '\n' && *c != '\r').collect();
    let mut result = String::new();
    for (count, c) in single_line.chars().enumerate() {
        if count >= max_chars {
            result.push('…');
            break;
        }
        result.push(c);
    }
    result
}

/// 构建对话历史 Section
pub fn conversation_section(
    messages: &[Message],
    accounts: &[Account],
    topics: &[Topic],
    bot_account_id: i64,
) -> Section {
    let msg_refs: Vec<&Message> = messages.iter().collect();
    let ctx = RenderContext::new(accounts, topics, messages, bot_account_id);
    let element = conversation_element(&msg_refs, &ctx);
    match element {
        RenderElement::Section(section) => section,
        other => Section::new("chat").add_child(other),
    }
}
