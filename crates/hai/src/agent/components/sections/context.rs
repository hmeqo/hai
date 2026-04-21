//! 上下文渲染
//!
//! 将 CommonContext（纯数据）渲染为 Section / prompt 字符串。
//! 此模块属于展示层（agent），负责将业务数据转换为 LLM 可消费的 XML 格式。

use crate::agent::components::RenderContext;
use crate::agent::render::{Format, Section, item, render_pretty, section};
use crate::domain::service::context::CommonContext;

use super::{
    account::account_element,
    chat::render_chat_info,
    conversation_element,
    memory::related_memories_section,
    topic::{topic_element, topic_search_section},
};

// ─── 上下文 Section 构建 ─────────────────────────────────────────────────────

/// 将通用上下文组装为顶层 Section
///
/// 阅读顺序：越静态、越宏观的记忆在上；越动态、越具体的最新消息在下。
///
/// 1. 情境（instruction）  — 为什么被唤醒
/// 2. 环境（info/chat/accounts）— 身份与场景背景
/// 3. 长期记忆（related_memories）— 最静态的背景知识
/// 4. 历史话题（related_topics / inactive_topics）— 过往上下文
/// 5. 当前话题（active_topics）— 正在进行的讨论
/// 6. 草稿板（scratchpad）— 自己的工作笔记
/// 7. 对话（conversation）— 最动态的最新消息，紧贴决策点
///
/// instruction 由调用方传入，插入在最前面
pub fn build_context_section(ctx: &CommonContext, instruction: Section) -> Section {
    let render_ctx = RenderContext::new(
        &ctx.accounts,
        &ctx.topics,
        &ctx.messages.messages,
        ctx.bot.account_id,
    );

    // ── 1. 环境 ────────────────────────────────────────────────────────
    let shown_unread = ctx
        .messages
        .messages
        .iter()
        .filter(|m| m.interaction_status == "pending")
        .count() as i64;
    let remaining = ctx.total_unread - shown_unread;

    let unread_hint = if remaining > 0 {
        format!(
            "{shown_unread} in window ({} total unread)",
            ctx.total_unread
        )
    } else {
        shown_unread.to_string()
    };

    let info_section = section("info")
        .with_item(item("current_time").with_content(&ctx.current_time))
        .with_item(
            item("you_are")
                .with_attr("id", ctx.bot.account_id)
                .with_attr("username", &ctx.bot.username)
                .with_attr("name", &ctx.bot.name),
        )
        .with_item(item("unread").with_content(unread_hint));

    let chat_section = render_chat_info(&ctx.chat);

    let accounts_section = section("accounts").add_children(
        ctx.accounts
            .iter()
            .filter(|a| a.id != ctx.bot.account_id)
            .map(account_element),
    );

    // ── 2. 对话 ────────────────────────────────────────────────────────
    let conversation_sec = build_conversation_section(ctx, &render_ctx);

    // ── 3. 知识 ────────────────────────────────────────────────────────
    let (active_topics_sec, inactive_topics_sec) = build_topic_sections(&ctx.topics, &render_ctx);

    let related_topics_sec = topic_search_section(&ctx.messages.related_topics, "related_topics");

    let related_memories_sec =
        related_memories_section(&ctx.messages.related_memories, "related_memories");

    // ── 4. 草稿板 ─────────────────────────────────────────────────────────────
    let scratchpad_section = ctx
        .scratchpad
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|s| section("scratchpad").add_child(item("content").with_content(&s.content)));

    // 静态→动态：宏观背景在上，最新消息在下
    let mut children = vec![
        instruction,
        info_section,
        chat_section,
        accounts_section,
        related_memories_sec,
        related_topics_sec,
    ];
    if let Some(inactive_sec) = inactive_topics_sec {
        children.push(inactive_sec);
    }
    children.push(active_topics_sec);
    if let Some(sp) = scratchpad_section {
        children.push(sp);
    }
    children.push(conversation_sec);

    section("context").add_children(children)
}

/// 构建对话 section
fn build_conversation_section(ctx: &CommonContext, render_ctx: &RenderContext) -> Section {
    let msg_refs: Vec<&_> = ctx.messages.messages.iter().collect();
    let element = conversation_element(&msg_refs, render_ctx);
    match element {
        crate::agent::render::elements::RenderElement::Section(section) => section,
        other => section("conversation").add_child(other),
    }
}

/// 话题不活跃阈值：超过此时间的话题标记为 inactive。
const TOPIC_INACTIVE_HOURS: f64 = 3.0;

/// 将话题按 last_active_at 分成 active / inactive 两组。
///
/// - `<active_topics>` —— 近期仍有活动的话题
/// - `<inactive_topics>` —— 超过阈值未活动，LLM 应考虑结项
///
/// 如果没有 inactive 话题则返回 None，避免空 section 占上下文。
fn build_topic_sections(
    topics: &[crate::domain::entity::Topic],
    render_ctx: &RenderContext,
) -> (Section, Option<Section>) {
    let now = jiff::Timestamp::now();
    let threshold = jiff::SignedDuration::from_hours(TOPIC_INACTIVE_HOURS as i64);
    let cutoff = now - threshold;

    let mut active = Vec::new();
    let mut inactive = Vec::new();

    for topic in topics {
        if topic.last_active_at() < cutoff {
            inactive.push(topic);
        } else {
            active.push(topic);
        }
    }

    let active_sec = section("active_topics").add_children(
        active
            .iter()
            .map(|t| topic_element(t, render_ctx))
            .collect::<Vec<_>>(),
    );

    let inactive_sec = if inactive.is_empty() {
        None
    } else {
        Some(
            section("inactive_topics")
                .with_attr("hint", "长时间未活跃，考虑结项")
                .add_children(
                    inactive
                        .iter()
                        .map(|t| topic_element(t, render_ctx))
                        .collect::<Vec<_>>(),
                ),
        )
    };

    (active_sec, inactive_sec)
}

// ─── Prompt 渲染 ──────────────────────────────────────────────────────────────

/// 将 CommonContext 渲染为最终的 XML prompt 字符串
pub fn render_main_context(ctx: &CommonContext, instruction: Section) -> String {
    render_pretty(build_context_section(ctx, instruction), Format::Xml)
}
