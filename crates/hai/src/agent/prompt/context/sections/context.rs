use super::{
    account::account_element,
    chat::{render_chat_info, render_current_chat},
    host_action::host_actions_element,
    memory::memory_element,
    message::conversation_element,
    perception::perception_item,
    topic::topic_element,
};
use crate::{
    agent::{events::EventGroup, prompt::context::RenderContext},
    agentcore::render::{Node, render_xml},
    domain::model::Memory,
};

/// Renders one `<context>` root, shared by the first and later turns of a chapter.
pub fn render_main_context(ctx: &RenderContext, instruction: Node) -> String {
    render_xml(
        &Node::tag("context").children(
            ContextBuilder::new(ctx, instruction)
                .env()
                .chat()
                .accounts()
                .current()
                .recall()
                .perceptions()
                .host_actions()
                .conversation()
                .build(),
        ),
    )
}

#[cfg(test)]
pub(crate) fn render_situation_only(groups: &[EventGroup]) -> String {
    render_xml(&Node::tag("context").child(build_situation_section(groups)))
}

/// The same `<context>` as `render_main_context` without the conversation: the environment, the
/// chat, its accounts, the current topics and memories, and the perceptions — everything but the
/// message list. A pass that reads the chat's own record does so through its tools, never by being
/// handed the messages.
pub(crate) fn render_context_for_digest(ctx: &RenderContext, instruction: Node) -> String {
    render_xml(
        &Node::tag("context").children(
            ContextBuilder::new(ctx, instruction)
                .env()
                .chat()
                .accounts()
                .current()
                .perceptions()
                .build(),
        ),
    )
}

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
        let mut env = Node::tag("environment");

        if self.ctx.first_turn {
            env = env.child(
                Node::tag("you_are")
                    .attr("id", self.ctx.channel.account_id.0)
                    .attr("username", &self.ctx.channel.username)
                    .attr("name", &self.ctx.channel.name),
            );
        }
        env = env.child(Node::tag("current_time").child(Node::text(&self.ctx.current_time)));

        self.add(env)
    }

    fn chat(self) -> Self {
        let current = self
            .ctx
            .chats
            .iter()
            .find(|chat| chat.id == self.ctx.origin_chat)
            .or(self.ctx.chat.as_ref());
        let Some(current) = current else {
            return self;
        };
        let mut this = self;
        if this.ctx.first_turn || this.ctx.roster_dirty {
            let roster: Vec<Node> = this.ctx.chats.iter().map(render_chat_info).collect();
            this = this.add(Node::tag("chats").children(roster));
        }
        this.add(render_current_chat(current))
    }

    fn accounts(self) -> Self {
        if !self.ctx.first_turn {
            return self;
        }
        let accounts: Vec<Node> = self
            .ctx
            .accounts
            .iter()
            .filter(|a| a.id != self.ctx.channel.account_id)
            .map(account_element)
            .collect();
        self.add(Node::tag("accounts").children(accounts))
    }

    fn current(self) -> Self {
        let ctx = self.ctx;
        let cutoff =
            jiff::Timestamp::now() - jiff::SignedDuration::from_hours(ctx.topic_idle_hours);
        let (active, stale): (Vec<_>, Vec<_>) = ctx
            .current_topics
            .iter()
            .partition(|topic| topic.last_active_at >= cutoff);

        let topics: Vec<Node> = active
            .iter()
            .map(|topic| topic_element(topic, false))
            .chain(stale.iter().map(|topic| topic_element(topic, true)))
            .collect();
        self.topics_and_memories("current", topics, &ctx.pinned_memories)
    }

    fn recall(self) -> Self {
        let ctx = self.ctx;
        let topics: Vec<Node> = ctx
            .recalled_topics
            .iter()
            .map(|topic| topic_element(topic, false))
            .collect();
        self.topics_and_memories("recall", topics, &ctx.recalled_memories)
    }

    fn topics_and_memories(self, tag: &str, topics: Vec<Node>, memories: &[Memory]) -> Self {
        let memories: Vec<Node> = memories.iter().map(memory_element).collect();
        if topics.is_empty() && memories.is_empty() {
            return self;
        }
        self.add(Node::tag(tag).children(topics).children(memories))
    }

    fn perceptions(self) -> Self {
        let nodes: Vec<Node> = self.ctx.perceptions().iter().map(perception_item).collect();
        self.add(Node::tag("perceptions").children(nodes))
    }

    fn conversation(self) -> Self {
        let msg_refs: Vec<&_> = self.ctx.messages.window().iter().collect();
        let node = conversation_element(&msg_refs, self.ctx);
        self.add(node)
    }

    /// What the host changed since the last Turn, rendered just before the conversation so the
    /// agent reads it as the state it is about to answer in.
    fn host_actions(self) -> Self {
        let node = host_actions_element(&self.ctx.host_actions, self.ctx.language);
        self.add(node)
    }

    fn build(self) -> Vec<Node> {
        self.children
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::{
        agent::{
            events::{WakeEvent, WakeEvents, WakeReason},
            prompt::{
                context::{ContextMessages, render_context::fixtures::render_context_data},
                text::TextCatalog,
            },
        },
        agentcore::render::elements::Node,
        config::conversation::Language,
        domain::{
            model::{Account, Memory, Message, Topic, TopicStatus},
            test_support,
            vo::{AccountId, AgentCommand, ChatId, HostAction, MessageId},
        },
    };

    #[test]
    fn situation_only_prompt_carries_the_wake_instruction() {
        let groups = [EventGroup {
            label: "scheduled",
            describe: "remind the user to take their medication".to_owned(),
            count: 1,
        }];
        let rendered = render_situation_only(&groups);

        assert!(rendered.contains("<situation>"), "{rendered}");
        assert!(rendered.contains("reason=\"scheduled\""), "{rendered}");
        assert!(
            rendered.contains("remind the user to take their medication"),
            "{rendered}"
        );
    }

    /// A command wake stores no message, so the instruction it carries has to be the whole turn.
    #[test]
    fn a_command_wake_renders_its_instruction_without_messages() {
        let reason = WakeReason::Command(AgentCommand::Explain(None));
        let instruction = AgentCommand::Explain(None).instruction(TextCatalog::Zh);
        let events = WakeEvents::new(vec![WakeEvent::new(reason.clone())]);

        let rendered = render_situation_only(&events.coalesce(Language::Zh));

        assert!(
            rendered.contains(&format!("reason=\"{}\"", reason.label())),
            "{rendered}"
        );
        assert!(rendered.contains(&instruction), "{rendered}");
    }

    fn memory(id: u128, content: &str) -> Memory {
        Memory {
            conversation_id: crate::domain::vo::ConversationId(2),
            created_at: jiff::Timestamp::now(),
            updated_at: jiff::Timestamp::now(),
            ..test_support::memory(id, content)
        }
    }

    fn topic(id: u128, title: &str, idle_hours: i64) -> Topic {
        let at = jiff::Timestamp::now() - jiff::SignedDuration::from_hours(idle_hours);
        Topic {
            conversation_id: crate::domain::vo::ConversationId(2),
            status: TopicStatus::Active,
            started_at: at,
            last_active_at: at,
            created_at: at,
            updated_at: at,
            ..test_support::topic(id, title)
        }
    }

    /// `<current>` holds the agent's own marks (open topics plus pinned memories) and `<recall>` holds
    /// what the system dredged; an idle-past-timeout topic inside `<current>` carries need-close.
    #[test]
    fn injection_block_splits_current_and_recall() {
        let mut data = render_context_data();
        data.current_topics = vec![
            topic(1, "what we're discussing", 0),
            topic(2, "something left hanging", 5),
        ];
        data.pinned_memories = vec![memory(10, "standing fact")];
        data.recalled_memories = vec![memory(11, "recalled memory")];
        data.recalled_topics = vec![topic(3, "something from before", 48)];

        let ctx = RenderContext::new(data);

        let rendered = render_main_context(&ctx, Node::tag("situation"));

        assert!(rendered.contains("<current>"), "{rendered}");
        assert!(rendered.contains("<recall>"), "{rendered}");
        assert!(rendered.contains("standing fact"), "{rendered}");
        assert!(rendered.contains("what we're discussing"), "{rendered}");
        assert!(rendered.contains("recalled memory"), "{rendered}");
        assert!(rendered.contains("something from before"), "{rendered}");
        // an idle-past-timeout topic carries need-close; an active one does not
        assert_eq!(rendered.matches("need-close").count(), 1, "{rendered}");
        // system-dredged entries stay in the recall section, not in current
        let current = rendered
            .split("<recall>")
            .next()
            .expect("current section before recall");
        assert!(!current.contains("recalled memory"), "{rendered}");
        assert!(!current.contains("something from before"), "{rendered}");
    }

    /// Dropping the conversation drops the messages and nothing else: the same context renders
    /// identically up to the conversation section and closes right after it.
    #[test]
    fn dropping_the_conversation_drops_only_the_messages() {
        let mut data = render_context_data();
        data.current_time = "2026-09-23T20:21:30+08:00".to_owned();
        data.messages = ContextMessages::new(
            vec![
                test_support::message("first thing said"),
                Message {
                    id: MessageId(2),
                    ..test_support::message("second thing said")
                },
            ],
            HashMap::new(),
        );
        data.current_topics = vec![topic(1, "what we're discussing", 0)];
        data.pinned_memories = vec![memory(10, "standing fact")];

        let ctx = RenderContext::new(data);
        let with = render_main_context(&ctx, Node::tag("situation"));
        let without = render_context_for_digest(&ctx, Node::tag("situation"));

        assert!(with.contains("<conversation"), "{with}");
        assert!(!without.contains("<conversation"), "{without}");
        assert!(
            with.contains("first thing said") && with.contains("second thing said"),
            "{with}"
        );

        for kept in [
            "<current_time>",
            "<current>",
            "standing fact",
            "what we're discussing",
        ] {
            assert!(
                with.contains(kept),
                "with the conversation, {kept} is missing"
            );
            assert!(
                without.contains(kept),
                "without the conversation, {kept} is missing"
            );
        }

        // The conversation is the last section, so the two renders run the same up to it and the one
        // without it closes right there.
        let head = with[..with.find("<conversation").expect("a rendered conversation")].trim_end();
        assert_eq!(
            without.trim_end(),
            format!("{head}\n</context>"),
            "the two differ by more than the conversation section"
        );
    }

    /// A later turn of a chapter renders no identity, profile or directory (every item appears once in the
    /// accumulated context) and refreshes current_time only.
    #[test]
    fn later_turn_renders_only_the_volatile_parts() {
        let data = |first_turn: bool| {
            let mut data = render_context_data();
            data.chat.as_mut().expect("fixture chat").name = Some("Test Group".into());
            std::sync::Arc::make_mut(&mut data.chats)[0].name = Some("Test Group".into());
            data.current_time = "2026-09-23T20:21:30+08:00".to_owned();
            data.first_turn = first_turn;
            data.accounts = vec![Account {
                id: AccountId(6),
                channel_id: crate::domain::vo::PlatformChannelId(1),
                external_id: "6".into(),
                meta: None,
                last_active_at: jiff::Timestamp::now(),
                created_at: jiff::Timestamp::now(),
                updated_at: jiff::Timestamp::now(),
            }];
            data
        };

        let first = render_main_context(&RenderContext::new(data(true)), Node::tag("situation"));
        assert!(first.contains("<you_are"), "{first}");
        assert!(first.contains("<accounts>"), "{first}");
        // an attribute-only node (chat) must not be dropped as empty
        assert!(first.contains("<chats>"), "{first}");
        assert!(first.contains("<chat alias=\"a\""), "{first}");
        assert!(first.contains("name=\"Test Group\""), "{first}");
        assert!(first.contains("<current_chat alias=\"a\""), "{first}");

        let later = render_main_context(&RenderContext::new(data(false)), Node::tag("situation"));
        assert!(!later.contains("<you_are"), "{later}");
        assert!(!later.contains("<accounts>"), "{later}");
        assert!(!later.contains("<chats>"), "{later}");
        assert!(later.contains("<current_chat alias=\"a\""), "{later}");
        assert!(later.contains("<current_time>"), "{later}");
    }

    /// A host action is context the agent reads before the conversation, in the context's language.
    #[test]
    fn host_actions_render_before_the_conversation() {
        let mut data = render_context_data();
        data.host_actions = vec![HostAction::PersonaChanged];
        data.language = Language::En;

        let rendered = render_main_context(&RenderContext::new(data), Node::tag("situation"));

        assert!(rendered.contains("<host_actions>"), "{rendered}");
        assert!(rendered.contains("<action kind=\"persona\">"), "{rendered}");
        assert!(
            rendered.contains("The user updated the persona."),
            "{rendered}"
        );
        let host = rendered
            .find("<host_actions>")
            .expect("a host action section");
        let conversation = rendered.find("<conversation").expect("a conversation");
        assert!(host < conversation, "{rendered}");
    }

    /// An unchanged setting renders no section at all.
    #[test]
    fn no_host_actions_renders_no_section() {
        let rendered = render_main_context(
            &RenderContext::new(render_context_data()),
            Node::tag("situation"),
        );
        assert!(!rendered.contains("<host_actions"), "{rendered}");
    }

    /// A membership change after the chapter opened re-renders the full roster, not a delta.
    #[test]
    fn a_dirty_roster_renders_on_a_later_turn() {
        let mut data = render_context_data();
        data.first_turn = false;
        data.roster_dirty = true;
        let mut second = data.chat.clone().expect("fixture chat");
        second.id = ChatId(3);
        second.alias = "b".into();
        data.chats = std::sync::Arc::from([data.chat.clone().expect("fixture chat"), second]);

        let rendered = render_main_context(&RenderContext::new(data), Node::tag("situation"));
        assert!(rendered.contains("<chats>"), "{rendered}");
        assert!(rendered.contains("<chat alias=\"a\""), "{rendered}");
        assert!(rendered.contains("<chat alias=\"b\""), "{rendered}");
        assert!(rendered.contains("<current_chat alias=\"a\""), "{rendered}");
    }
}
