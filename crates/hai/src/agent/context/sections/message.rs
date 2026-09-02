use crate::{
    agent::context::{
        RenderContext,
        fmt::{format_date_label, format_time_only},
    },
    agentcore::render::elements::Node,
    domain::model::{Message, MessageStatus},
};

pub(crate) fn message_element(msg: &Message, ctx: &RenderContext) -> Node {
    let own = msg.account_id == Some(ctx.bot.account_id);

    let mut b = Node::tag("msg")
        .attr("id", msg.id)
        .attr("from", ctx.sender_name(msg))
        .attr("at", format_time_only(msg.sent_at));

    if own {
        b = b.attr("own", true);
    }

    if let Some(reply_id) = msg.reply_to_id
        && let Some(replied) = ctx.messages.get(reply_id)
    {
        let replied_msg = replied.message();
        let replied_sender = ctx.sender_name(replied_msg);

        let mut elements = (ctx.content_renderer)(&replied_msg.content);
        // 窗口内引用内容已作为独立消息可见 → 截断；窗口外引用是唯一呈现 → 保留全文。
        if replied.is_in_window() {
            truncate_text_nodes(&mut elements, 50);
        }

        let mut reference = Node::tag("reference")
            .attr("id", reply_id)
            .attr("from", replied_sender.as_str())
            .attr("at", format_time_only(replied_msg.sent_at));

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
            root = root.child(Node::tag("separator").child(Node::text("新消息")));
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

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use serde_json::json;

    use super::*;
    use crate::{
        agent::{
            context::{
                ContextMessages,
                render_context::{ContentRenderer, RenderContextData},
            },
            link::BotProfile,
        },
        agentcore::render::{Format, render_pretty},
        domain::model::Chat,
    };

    fn msg(id: i64, account_id: Option<i64>, reply_to_id: Option<i64>, text: &str) -> Message {
        Message {
            id,
            chat_id: 2,
            account_id,
            role: "user".into(),
            content: json!({ "text": text }),
            topic_id: None,
            interaction_status: MessageStatus::Seen.as_str().into(),
            reply_to_id,
            external_id: None,
            meta: None,
            sent_at: Some(jiff::Timestamp::now()),
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
        }
    }

    fn render_ctx(messages: Vec<Message>, reply_context: HashMap<i64, Message>) -> RenderContext {
        let renderer: ContentRenderer = Arc::new(|content| {
            let text = content.get("text").and_then(|t| t.as_str()).unwrap_or("");
            vec![Node::text(text)]
        });
        RenderContext::new(
            RenderContextData {
                bot: BotProfile {
                    account_id: 1,
                    username: "bot".into(),
                    name: "Bot".into(),
                },
                chat: Chat {
                    id: 2,
                    platform: "telegram".into(),
                    external_id: "2".into(),
                    chat_type: "group".into(),
                    name: None,
                    meta: None,
                    created_at: jiff::Timestamp::now(),
                    updated_at: jiff::Timestamp::now(),
                },
                current_time: String::new(),
                total_unread: 0,
                messages: ContextMessages::new(messages, reply_context),
                accounts: Vec::new(),
                topics: Vec::new(),
                related_topics: Vec::new(),
                related_memories: Vec::new(),
                related_knowledge: Vec::new(),
                perceptions: Vec::new(),
                topic_idle_hours: 0,
            },
            renderer,
        )
    }

    /// 窗口外引用目标只进 reference（内容可见），不渲染为独立 `<msg>`——消除跨轮重复渲染。
    #[test]
    fn out_of_window_reference_not_rendered_as_message() {
        let long_text = "这是一条超过五十个字符的完整引用内容用来验证窗口外引用不会被截断保留全文可见性测试文本加上额外的字符确保一定超过五十个字符的长度";
        let replied = msg(37575, Some(6), None, long_text);
        let reply = msg(37576, Some(1), Some(37575), "最近主要在聊域名续费");
        let sticker = msg(37577, Some(4), None, "");

        let ctx = render_ctx(
            vec![reply.clone(), sticker.clone()],
            HashMap::from([(37575, replied)]),
        );
        let node = conversation_element(&[&reply, &sticker], &ctx);
        let out = render_pretty(node, Format::Xml);

        // reference 有内容，且不截断（窗口外引用是唯一呈现渠道——全文可见）
        assert!(out.contains("<reference id=\"37575\""));
        assert!(out.contains(long_text));
        assert!(!out.contains("…"));
        // 引用目标不独立渲染；窗口消息正常渲染
        assert!(!out.contains("<msg id=\"37575\""));
        assert!(out.contains("<msg id=\"37576\""));
        assert!(out.contains("<msg id=\"37577\""));
    }

    /// 窗口内引用保持原语义：引用目标作为消息渲染 + reference 并存（非重复，是引用关系），
    /// reference 截断 50 字符（内容已作为独立消息可见）。
    #[test]
    fn in_window_reference_renders_message_and_reference() {
        let long_text = "这是一条超过五十个字符的完整引用内容用来验证窗口内引用会被截断处理测试文本加上额外的字符确保一定超过五十个字符的长度";
        let replied = msg(37575, Some(6), None, long_text);
        let reply = msg(37576, Some(1), Some(37575), "回复");

        let ctx = render_ctx(vec![replied.clone(), reply.clone()], HashMap::new());
        let node = conversation_element(&[&replied, &reply], &ctx);
        let out = render_pretty(node, Format::Xml);

        assert!(out.contains("<msg id=\"37575\""));
        assert!(out.contains("<reference id=\"37575\""));
        // 独立消息全文可见；reference 截断（50 字符 + …）
        assert!(out.contains(long_text));
        assert!(out.contains("…"));
    }
}
