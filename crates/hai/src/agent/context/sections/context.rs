//! 上下文渲染
//!
//! 将 CommonContext（纯数据）渲染为 prompt 字符串。

use super::{
    account::account_element,
    chat::render_chat_info,
    memory::related_memories_section,
    message::conversation_element,
    perception::perception_item,
    topic::{topic_element, topic_element_static},
};
use crate::{
    agent::{context::RenderContext, event::WakeEvent},
    agentcore::render::{Format, Node, render_pretty},
    domain::{entity::MessageStatus, vo::Source},
};

/// 将 CommonContext 渲染为最终的 XML prompt 字符串
pub fn render_main_context(ctx: &RenderContext, instruction: Node) -> String {
    render_pretty(build_context_section(ctx, instruction), Format::Xml)
}

/// 构建 `<situation>` section（描述唤醒原因）
pub fn build_situation_section(events: &[WakeEvent]) -> Node {
    if events.is_empty() {
        return Node::tag("situation");
    }

    Node::tag("situation").children(
        events
            .iter()
            .map(|event| {
                Node::tag("trigger")
                    .attr("reason", event.reason.label())
                    .child(Node::text(event.reason.describe()))
            })
            .collect::<Vec<_>>(),
    )
}

/// 将通用上下文组装为顶层 Context 节点
///
/// 阅读顺序：越静态、越宏观的记忆在上；越动态、越具体的最新消息在下。
///
/// 1. instruction  — 为什么被唤醒
/// 2. environment  — 身份与场景背景
/// 3. chat         — 聊天信息
/// 4. accounts     — 参与者列表
/// 5. related_memories — 最静态的背景知识
/// 6. related_topics — 过往上下文
/// 7. current_topics — 当前话题（含 idle 属性）
/// 8. scratchpad   — 上次的思路延续
/// 9. perceptions  — 附件分析结果
/// 10. conversation — 最动态的最新消息
pub fn build_context_section(ctx: &RenderContext, instruction: Node) -> Node {
    let env_section = build_env_section(ctx);
    let chat_section = render_chat_info(&ctx.chat);
    let accounts_section = Node::tag("accounts").children(
        ctx.accounts
            .iter()
            .filter(|a| a.id != ctx.bot.account_id)
            .map(account_element)
            .collect::<Vec<_>>(),
    );

    let related_memories_sec = related_memories_section(&ctx.related_memories, "related_memories");
    let related_topics_sec = Node::tag("related_topics").children(
        ctx.related_topics
            .iter()
            .map(|r| topic_element_static(&r.topic).attr("relevance", format!("{:.4}", r.distance)))
            .collect::<Vec<_>>(),
    );

    let topics_sec = build_topics_section(ctx);

    let conversation_sec = {
        let msg_refs: Vec<&_> = ctx.messages.iter().collect();
        conversation_element(&msg_refs, ctx)
    };

    let perceptions_sec = build_perceptions_section(ctx);

    let scratchpad_sec = ctx
        .scratchpad
        .as_ref()
        .map(|note| Node::tag("scratchpad").with_text(note));

    let mut children: Vec<Node> = vec![instruction, env_section, chat_section];
    push_non_empty(&mut children, accounts_section);
    push_non_empty(&mut children, related_memories_sec);
    push_non_empty(&mut children, related_topics_sec);
    if let Some(topics) = topics_sec {
        children.push(topics);
    }
    if let Some(sp) = scratchpad_sec {
        children.push(sp);
    }
    push_non_empty(&mut children, perceptions_sec);
    children.push(conversation_sec);
    Node::tag("context").children(children)
}

fn build_env_section(ctx: &RenderContext) -> Node {
    let mut env = Node::tag("environment")
        .child(
            Node::tag("you_are")
                .attr("id", ctx.bot.account_id)
                .attr("username", &ctx.bot.username)
                .attr("name", &ctx.bot.name),
        )
        .child(Node::tag("current_time").child(Node::text(&ctx.current_time)));

    let shown_unread = ctx
        .messages
        .iter()
        .filter(|m| m.interaction_status == MessageStatus::Unread.as_str())
        .count() as i64;
    let remaining = ctx.total_unread - shown_unread;
    if remaining > 0 {
        env = env.child(Node::tag("unread").child(Node::text(format!(
            "{shown_unread} in window ({} total unread)",
            ctx.total_unread
        ))));
    }

    if let Some(sandbox) = &ctx.sandbox_info
        && sandbox.enabled
    {
        env = env.child(
            Node::tag("sandbox")
                .attr("enabled", "true")
                .attr("runtime", &sandbox.runtime)
                .attr("image", &sandbox.image),
        );
    }

    env
}

fn build_topics_section(ctx: &RenderContext) -> Option<Node> {
    let cutoff = jiff::Timestamp::now() - jiff::SignedDuration::from_hours(ctx.topic_idle_hours);

    let (active, stale): (Vec<_>, Vec<_>) = ctx
        .topics
        .iter()
        .partition(|t| t.last_active_at() >= cutoff);

    if active.is_empty() && stale.is_empty() {
        return None;
    }

    Some(
        Node::tag("current_topics")
            .children(
                active
                    .iter()
                    .map(|t| topic_element(t, false))
                    .collect::<Vec<_>>(),
            )
            .children(
                stale
                    .iter()
                    .map(|t| topic_element(t, true))
                    .collect::<Vec<_>>(),
            ),
    )
}

fn build_perceptions_section(ctx: &RenderContext) -> Node {
    // Resource 来源的 perception 已嵌入 attachment，这里只展示 URL 来源的
    let url_perceptions: Vec<_> = ctx
        .perceptions()
        .iter()
        .filter(|p| matches!(p.source(), Some(Source::Url { .. })))
        .collect();

    Node::tag("perceptions").children(
        url_perceptions
            .iter()
            .map(|p| perception_item(p))
            .collect::<Vec<_>>(),
    )
}

fn push_non_empty(children: &mut Vec<Node>, node: Node) {
    if !node.is_empty() {
        children.push(node);
    }
}
