//! 消息组件 - 构建 RenderElement

use std::collections::HashSet;

use crate::{
    agent::{
        context::{RenderContext, render_context::format_time_dyn2},
        render::content,
    },
    agentcore::render::elements::{RenderElement, Section, item, section},
    domain::entity::{Message, MessageStatus},
};

/// 构建单条消息元素（不含 sender / topic，由外层分组提供）
pub fn message_element(msg: &Message, ctx: &RenderContext) -> RenderElement {
    let sent_at = format_time_dyn2(msg.sent_at());
    let is_replied = msg.status() == MessageStatus::Replied;

    let mut builder = item("message")
        .with_attr("id", msg.id)
        .with_attr("role", &msg.role)
        .with_attr("sent_at", sent_at.as_str())
        .with_attr("replied", is_replied);

    if let Some(reply_id) = msg.reply_to_id
        && let Some(replied_msg) = ctx.get_message(reply_id)
    {
        let replied_sender = ctx.sender_name(replied_msg);
        let replied_sent_at = format_time_dyn2(replied_msg.sent_at());
        let replied_is_replied = replied_msg.status() == MessageStatus::Replied;

        let mut elements = content::render_content(
            &replied_msg.content,
            &ctx.data.perception_by_attachment_id,
            &ctx.data.same_resource_as,
        );
        truncate_text_nodes(&mut elements, 50);

        let mut reply = item("reply")
            .with_attr("id", reply_id)
            .with_attr("role", &replied_msg.role)
            .with_attr("sender", replied_sender.as_str())
            .with_attr("sent_at", replied_sent_at.as_str())
            .with_attr("replied", replied_is_replied);

        reply = reply.add_children(elements);

        builder = builder.add_child(reply);
    }

    let content_elements = content::render_content(
        &msg.content,
        &ctx.data.perception_by_attachment_id,
        &ctx.data.same_resource_as,
    );
    builder.add_children(content_elements).into_element()
}

/// 构建分组后的消息列表元素（按 topic → sender 层级包裹）
pub fn messages_elements(messages: &[&Message], ctx: &RenderContext) -> Vec<RenderElement> {
    group_messages(messages, ctx)
}

/// 按 topic → sender 分组包裹消息
///
/// ```xml
/// <topic title="闲聊">
///   <sender name="Alice (@alice)">
///     <message id="1" role="user" sent_at="...">...</message>
///     <message id="2" role="user" sent_at="...">...</message>
///   </sender>
///   <sender name="Bob (@bob)">
///     <message id="3" role="user" sent_at="...">...</message>
///   </sender>
/// </topic>
/// <sender name="Charlie">
///   <message id="4" role="assistant" sent_at="...">...</message>
/// </sender>
/// ```
fn group_messages(messages: &[&Message], ctx: &RenderContext) -> Vec<RenderElement> {
    let mut result = Vec::new();
    let mut i = 0;

    while i < messages.len() {
        let topic_hint = ctx.topic_hint(messages[i]);

        let mut j = i + 1;
        while j < messages.len() && ctx.topic_hint(messages[j]) == topic_hint {
            j += 1;
        }

        let topic_msgs = &messages[i..j];
        let mut sender_groups = Vec::new();
        let mut si = 0;

        while si < topic_msgs.len() {
            let sender = ctx.sender_name(topic_msgs[si]);
            let mut sj = si + 1;
            while sj < topic_msgs.len() && ctx.sender_name(topic_msgs[sj]) == sender {
                sj += 1;
            }

            let msg_els: Vec<RenderElement> = topic_msgs[si..sj]
                .iter()
                .map(|m| message_element(m, ctx))
                .collect();

            sender_groups.push(
                section("sender")
                    .with_attr("name", sender.as_str())
                    .add_children(msg_els)
                    .into_element(),
            );

            si = sj;
        }

        if !topic_hint.is_empty() {
            result.push(
                section("topic")
                    .with_attr("title", topic_hint.trim())
                    .add_children(sender_groups)
                    .into_element(),
            );
        } else {
            result.extend(sender_groups);
        }

        i = j;
    }

    result
}

/// 构建消息 Section
pub fn messages_section(messages: &[&Message], ctx: &RenderContext, tag: &str) -> Section {
    section(tag).add_children(messages_elements(messages, ctx))
}

/// 构建对话历史元素（含 history / unread / reply_context 分区）
pub fn conversation_element(messages: &[&Message], ctx: &RenderContext) -> RenderElement {
    if messages.is_empty() {
        return RenderElement::Empty;
    }

    let main_ids: HashSet<i64> = messages.iter().map(|m| m.id).collect();

    // reply_context：被引用但不在主窗口的消息
    let mut seen_reply_ids = HashSet::<i64>::new();
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
        root.push_child(
            section("reply_context")
                .add_children(messages_elements(reply_context_msgs.as_slice(), ctx)),
        );
    }

    root.push_child(
        section("history")
            .with_attr("limit", history.len() as i64)
            .add_children(messages_elements(&history, ctx)),
    );

    if !unread.is_empty() {
        root.push_child(section("unread").add_children(messages_elements(&unread, ctx)));
    }

    root.into_element()
}

/// 截断元素树中所有 Text 节点的内容，保留其他结构（attachment 等）
fn truncate_text_nodes(elements: &mut [RenderElement], max_chars: usize) {
    for el in elements.iter_mut() {
        if let RenderElement::Text(t) = el
            && t.content.chars().count() > max_chars
        {
            t.content = t.content.chars().take(max_chars).collect::<String>() + "…";
        }
    }
}
