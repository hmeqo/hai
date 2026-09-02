//! 上下文渲染
//!
//! 将 RenderContext（纯数据）渲染为 prompt 字符串。

use super::{
    account::account_element,
    chat::render_chat_info,
    knowledge::related_knowledge_section,
    memory::related_memories_section,
    message::conversation_element,
    perception::perception_item,
    topic::{topic_element, topic_element_static},
};
use crate::{
    agent::{context::RenderContext, event::EventGroup},
    agentcore::render::{Format, Node, render_pretty},
    domain::{model::MessageStatus, vo::Source},
};

pub fn render_main_context(ctx: &RenderContext, instruction: Node) -> String {
    render_context(ctx, instruction)
}

/// 首轮/后续轮统一 root = `context`。
pub(crate) fn render_context(ctx: &RenderContext, instruction: Node) -> String {
    render_pretty(
        Node::tag("context").children(
            ContextBuilder::new(ctx, instruction)
                .env()
                .chat()
                .accounts()
                .related_memories()
                .knowledge()
                .related_topics()
                .topics()
                .perceptions()
                .conversation()
                .build(),
        ),
        Format::Xml,
    )
}

/// 构建 `<situation>` section（描述唤醒原因）
pub fn build_situation_section(groups: &[EventGroup]) -> Node {
    if groups.is_empty() {
        return Node::tag("situation");
    }

    let children: Vec<Node> = groups
        .iter()
        .map(|g| {
            let mut n = Node::tag("trigger")
                .attr("reason", g.label)
                .child(Node::text(&g.describe));
            if g.count > 1 {
                n = n.attr("count", g.count.to_string());
            }
            n
        })
        .collect();

    Node::tag("situation").children(children)
}

struct ContextBuilder<'a> {
    ctx: &'a RenderContext,
    children: Vec<Node>,
}

impl<'a> ContextBuilder<'a> {
    fn new(ctx: &'a RenderContext, instruction: Node) -> Self {
        ContextBuilder {
            ctx,
            children: vec![instruction],
        }
    }

    fn add(mut self, node: Node) -> Self {
        if !node.is_empty() {
            self.children.push(node);
        }
        self
    }

    fn env(self) -> Self {
        let mut env = Node::tag("environment")
            .child(
                Node::tag("you_are")
                    .attr("id", self.ctx.bot.account_id)
                    .attr("username", &self.ctx.bot.username)
                    .attr("name", &self.ctx.bot.name),
            )
            .child(Node::tag("current_time").child(Node::text(&self.ctx.current_time)));

        let shown_unread = self
            .ctx
            .messages
            .window()
            .iter()
            .filter(|m| m.interaction_status == MessageStatus::Unread.as_str())
            .count() as i64;
        let remaining = self.ctx.total_unread - shown_unread;
        if remaining > 0 {
            env = env.child(Node::tag("unread").child(Node::text(format!(
                "{shown_unread} in window ({} total unread)",
                self.ctx.total_unread
            ))));
        }

        self.add(env)
    }

    fn chat(self) -> Self {
        let node = render_chat_info(&self.ctx.chat);
        self.add(node)
    }

    fn accounts(self) -> Self {
        let accounts: Vec<Node> = self
            .ctx
            .accounts
            .iter()
            .filter(|a| a.id != self.ctx.bot.account_id)
            .map(account_element)
            .collect();
        self.add(Node::tag("accounts").children(accounts))
    }

    fn related_memories(self) -> Self {
        let node = related_memories_section(&self.ctx.related_memories, "related_memories");
        self.add(node)
    }

    fn knowledge(self) -> Self {
        let node = related_knowledge_section(&self.ctx.related_knowledge);
        self.add(node)
    }

    fn related_topics(self) -> Self {
        let topics: Vec<Node> = self
            .ctx
            .related_topics
            .iter()
            .map(|r| topic_element_static(&r.topic).attr("relevance", format!("{:.4}", r.distance)))
            .collect();
        self.add(Node::tag("related_topics").children(topics))
    }

    fn topics(self) -> Self {
        let cutoff =
            jiff::Timestamp::now() - jiff::SignedDuration::from_hours(self.ctx.topic_idle_hours);

        let (active, stale): (Vec<_>, Vec<_>) = self
            .ctx
            .topics
            .iter()
            .partition(|t| t.last_active_at >= cutoff);

        if active.is_empty() && stale.is_empty() {
            return self;
        }

        self.add(
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

    fn perceptions(self) -> Self {
        let url_perceptions: Vec<_> = self
            .ctx
            .perceptions()
            .iter()
            .filter(|p| matches!(p.source(), Some(Source::Url { .. })))
            .collect();
        let nodes: Vec<Node> = url_perceptions.iter().map(|p| perception_item(p)).collect();
        self.add(Node::tag("perceptions").children(nodes))
    }

    fn conversation(self) -> Self {
        let msg_refs: Vec<&_> = self.ctx.messages.window().iter().collect();
        let node = conversation_element(&msg_refs, self.ctx);
        self.add(node)
    }

    fn build(self) -> Vec<Node> {
        self.children
    }
}
