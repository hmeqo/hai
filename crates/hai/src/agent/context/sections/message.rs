use crate::{
    agent::context::{
        RenderContext,
        fmt::{format_date_label, format_time_only},
    },
    agentcore::render::elements::Node,
    domain::model::{Message, MessageStatus},
};

pub(crate) fn message_element(msg: &Message, ctx: &RenderContext) -> Node {
    let is_replied = msg.status() == Some(MessageStatus::Replied);
    let own = msg.account_id == Some(ctx.bot.account_id);
    let is_new = msg.interaction_status == MessageStatus::Unread.as_str();

    let mut b = Node::tag("msg")
        .attr("id", msg.id)
        .attr("from", ctx.sender_name(msg))
        .attr("at", format_time_only(msg.sent_at));

    if is_replied {
        b = b.attr("replied", true);
    }
    if own {
        b = b.attr("own", true);
    }
    if is_new {
        b = b.attr("new", true);
    }

    if let Some(reply_id) = msg.reply_to_id
        && let Some(replied_msg) = ctx.get_message(reply_id)
    {
        let replied_sender = ctx.sender_name(replied_msg);
        let replied_is_replied = replied_msg.status() == Some(MessageStatus::Replied);

        let mut elements = (ctx.content_renderer)(&replied_msg.content);
        truncate_text_nodes(&mut elements, 50);

        let mut reference = Node::tag("reference")
            .attr("id", reply_id)
            .attr("from", replied_sender.as_str())
            .attr("at", format_time_only(replied_msg.sent_at));

        if replied_is_replied {
            reference = reference.attr("replied", true);
        }

        reference = reference.children(elements);
        b = b.child(reference);
    }

    let content_elements = (ctx.content_renderer)(&msg.content);
    b.children(content_elements)
}

pub fn conversation_element(messages: &[&Message], ctx: &RenderContext) -> Node {
    if messages.is_empty() {
        return Node::tag("conversation").attr("total", 0);
    }

    let first_unread = messages
        .iter()
        .position(|m| m.interaction_status == MessageStatus::Unread.as_str());

    let mut root = Node::tag("conversation").attr("total", messages.len() as i64);
    let mut prev_date: Option<String> = None;

    for (i, msg) in messages.iter().enumerate() {
        let label = format_date_label(msg.sent_at);
        if prev_date.as_ref() != Some(&label) {
            root = root.child(Node::tag("date").attr("value", &label));
            prev_date = Some(label);
        }

        if Some(i) == first_unread {
            root = root.child(Node::tag("separator"));
        }

        root = root.child(message_element(msg, ctx));
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
