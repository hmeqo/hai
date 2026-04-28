//! 上下文渲染
//!
//! 将 CommonContext（纯数据）渲染为 prompt 字符串。

use super::{
    account::account_element, chat::render_chat_info, memory::related_memories_section,
    message::conversation_element, topic::topic_element,
};
use crate::{
    agent::{
        context::{RenderContext, topic_element_static},
        render::content::perception_item,
    },
    agentcore::render::{Format, Section, item, render_pretty, section},
    domain::vo::Source,
};

/// 将 CommonContext 渲染为最终的 XML prompt 字符串
pub fn render_main_context(ctx: &RenderContext, instruction: Section) -> String {
    render_pretty(build_context_section(ctx, instruction), Format::Xml)
}

/// 将通用上下文组装为顶层 Section
///
/// 阅读顺序：越静态、越宏观的记忆在上；越动态、越具体的最新消息在下。
///
/// 1. instruction  — 为什么被唤醒
/// 2. environment  — 身份与场景背景
/// 3. related_memories — 最静态的背景知识
/// 4. related_topics — 过往上下文
/// 5. current_topics — 当前话题（含 idle 属性）
/// 6. scratchpad   — 上次的思路延续（先回顾自己）
/// 7. perceptions  — 附件分析结果（帮助理解下文消息）
/// 8. conversation — 最动态的最新消息
pub fn build_context_section(ctx: &RenderContext, instruction: Section) -> Section {
    let env_section = build_env_section(ctx);
    let chat_section = render_chat_info(&ctx.chat);
    let accounts_section = section("accounts").add_children(
        ctx.accounts
            .iter()
            .filter(|a| a.id != ctx.bot.account_id())
            .map(account_element),
    );

    let related_memories_sec = related_memories_section(&ctx.related_memories, "related_memories");
    let related_topics_sec = {
        let els = ctx.related_topics.iter().map(|r| {
            topic_element_static(&r.topic).with_attr("relevance", format!("{:.4}", r.distance))
        });
        section("related_topics").add_children(els)
    };

    let topics_sec = build_topics_section(ctx);

    let scratchpad_sec = ctx
        .scratchpad
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|s| section("scratchpad").add_child(item("content").with_content(&s.content)));

    let conversation_sec = {
        let msg_refs: Vec<&_> = ctx.messages.iter().collect();
        match conversation_element(&msg_refs, ctx) {
            crate::agentcore::render::elements::RenderElement::Section(s) => s,
            other => section("conversation").add_child(other),
        }
    };

    let perceptions_sec = build_perceptions_section(ctx);

    let mut children: Vec<Section> = vec![instruction, env_section, chat_section];
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
    section("context").add_children(children)
}

fn build_env_section(ctx: &RenderContext) -> Section {
    let mut env = section("environment")
        .with_item(
            item("you_are")
                .with_attr("id", ctx.bot.account_id())
                .with_attr("username", &ctx.bot.username)
                .with_attr("name", &ctx.bot.name),
        )
        .with_item(item("current_time").with_content(&ctx.current_time));

    let shown_unread = ctx
        .messages
        .iter()
        .filter(|m| m.interaction_status == "pending")
        .count() as i64;
    let remaining = ctx.total_unread - shown_unread;
    if remaining > 0 {
        env = env.with_item(item("unread").with_content(format!(
            "{shown_unread} in window ({} total unread)",
            ctx.total_unread
        )));
    }

    env
}

/// 话题闲置阈值：超过此时间的话题标记为 idle
const TOPIC_IDLE_HOURS: i64 = 3;

/// 将话题合并为 current_topics，有闲置话题时加 idle 属性
fn build_topics_section(ctx: &RenderContext) -> Option<Section> {
    let cutoff = jiff::Timestamp::now() - jiff::SignedDuration::from_hours(TOPIC_IDLE_HOURS);

    let (active, idle): (Vec<_>, Vec<_>) = ctx
        .topics
        .iter()
        .partition(|t| t.last_active_at() >= cutoff);

    if active.is_empty() && idle.is_empty() {
        return None;
    }

    Some(
        section("current_topics")
            .add_children(active.iter().map(|t| topic_element(t, false)))
            .add_children(idle.iter().map(|t| topic_element(t, true))),
    )
}

fn build_perceptions_section(ctx: &RenderContext) -> Section {
    // Resource 来源的 perception 已嵌入 attachment，这里只展示 URL 来源的
    let url_perceptions: Vec<_> = ctx
        .perceptions()
        .iter()
        .filter(|p| matches!(p.source(), Some(Source::Url { .. })))
        .collect();

    section("perceptions").add_children(url_perceptions.iter().map(|p| perception_item(p)))
}

fn push_non_empty(children: &mut Vec<Section>, section: Section) {
    if !section.is_empty() {
        children.push(section);
    }
}
