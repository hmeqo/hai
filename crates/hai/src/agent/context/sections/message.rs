use std::collections::HashSet;

use crate::{
    agent::context::{RenderContext, fmt::format_time_dyn2},
    agentcore::render::elements::Node,
    domain::model::{Message, MessageStatus},
};

pub(crate) fn message_element(msg: &Message, ctx: &RenderContext) -> Node {
    let sent_at = format_time_dyn2(msg.sent_at);
    let is_replied = msg.status() == MessageStatus::Replied;

    let mut b = Node::tag("message")
        .attr("id", msg.id)
        .attr("sent_at", sent_at.as_str());

    if is_replied {
        b = b.attr("replied", true);
    }

    if let Some(reply_id) = msg.reply_to_id
        && let Some(replied_msg) = ctx.get_message(reply_id)
    {
        let replied_sender = ctx.sender_name(replied_msg);
        let replied_sent_at = format_time_dyn2(replied_msg.sent_at);
        let replied_is_replied = replied_msg.status() == MessageStatus::Replied;

        let mut elements = (ctx.content_renderer)(&replied_msg.content);
        truncate_text_nodes(&mut elements, 50);

        let mut reference = Node::tag("reply_to")
            .attr("id", reply_id)
            .attr("sender", replied_sender.as_str())
            .attr("sent_at", replied_sent_at.as_str());

        if replied_is_replied {
            reference = reference.attr("replied", true);
        }

        reference = reference.children(elements);
        b = b.child(reference);
    }

    let content_elements = (ctx.content_renderer)(&msg.content);
    b.children(content_elements)
}

pub(crate) fn messages_elements(messages: &[&Message], ctx: &RenderContext) -> Vec<Node> {
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

            let msg_nodes: Vec<Node> = topic_msgs[si..sj]
                .iter()
                .map(|m| message_element(m, ctx))
                .collect();

            sender_groups.push(
                Node::tag("sender")
                    .attr("name", sender.as_str())
                    .children(msg_nodes),
            );

            si = sj;
        }

        if !topic_hint.is_empty() {
            result.push(
                Node::tag("topic")
                    .attr("title", topic_hint.trim())
                    .children(sender_groups),
            );
        } else {
            result.extend(sender_groups);
        }

        i = j;
    }

    result
}

pub fn conversation_element(messages: &[&Message], ctx: &RenderContext) -> Node {
    if messages.is_empty() {
        return Node::tag("conversation");
    }

    let main_ids: HashSet<i64> = messages.iter().map(|m| m.id).collect();

    let mut seen_reply_ids = HashSet::<i64>::new();
    let reply_context_msgs: Vec<&Message> = messages
        .iter()
        .filter_map(|m| m.reply_to_id)
        .filter(|rid| !main_ids.contains(rid) && seen_reply_ids.insert(*rid))
        .filter_map(|rid| ctx.get_message(rid))
        .collect();

    let (history, unread): (Vec<_>, Vec<_>) = messages
        .iter()
        .partition(|m| m.interaction_status != MessageStatus::Unread.as_str());

    let mut root = Node::tag("conversation");

    if !reply_context_msgs.is_empty() {
        root = root.child(
            Node::tag("reply_context")
                .children(messages_elements(reply_context_msgs.as_slice(), ctx)),
        );
    }

    root = root.child(
        Node::tag("history")
            .attr("limit", history.len() as i64)
            .children(messages_elements(&history, ctx)),
    );

    if !unread.is_empty() {
        root = root.child(Node::tag("unread").children(messages_elements(&unread, ctx)));
    }

    root
}

fn truncate_text_nodes(elements: &mut [Node], max_chars: usize) {
    for el in elements.iter_mut() {
        if let Node::Text(t) = el
            && t.chars().count() > max_chars
        {
            *t = t.chars().take(max_chars).collect::<String>() + "…";
        }
    }
}
