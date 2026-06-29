use std::collections::HashSet;

use uuid::Uuid;

use crate::{
    agent::{
        context::{
            RenderContext, build_situation_section,
            helper::{
                build_attachment_maps, collect_accounts, load_chat, load_perceptions,
                load_reply_context, search_related_context, search_related_dedup,
            },
            render_context::{RenderContextData, SandboxInfo},
            render_main_context,
            sections::{
                chat::render_chat_info, memory::related_memories_section,
                message::conversation_element, topic::topic_element_static,
            },
        },
        link::BuiltContext,
        runtime::ctx::RoundContext,
    },
    agentcore::render::{Format, Node, render_pretty},
    domain::model::Message,
    error::Result,
};

// ── Shared data ──────────────────────────────────────────────────────────────

struct RoundData {
    all_messages: Vec<Message>,
    message_ids: Vec<i64>,
    parsed: Vec<crate::agent::context::types::ParsedContent>,
    perception: crate::agent::context::types::PerceptionResult,
    topics: Vec<crate::domain::model::Topic>,
    accounts: Vec<crate::domain::model::Account>,
    chat: crate::domain::model::Chat,
}

async fn prepare_round_data(ctx: &RoundContext, messages: &[Message]) -> Result<RoundData> {
    let services = &ctx.app.db.srv;
    let parser = ctx.bot.handler.content_parser();
    let chat_id = ctx.chat_id;

    let (all_messages, message_ids) = prepare_messages(services, messages).await?;
    let parsed: Vec<_> = all_messages
        .iter()
        .map(|m| parser.parse(&m.content))
        .collect();
    let perception = load_perceptions(services, &parsed).await?;
    let topics = services.topic.get_active_topics(chat_id).await?;
    let accounts = collect_accounts(services, &all_messages).await?;
    let chat = load_chat(services, chat_id).await?;

    Ok(RoundData {
        all_messages,
        message_ids,
        parsed,
        perception,
        topics,
        accounts,
        chat,
    })
}

/// 加载 reply context → 合并 → 排序 → 提取 message_ids
async fn prepare_messages(
    services: &crate::domain::service::DbServices,
    messages: &[Message],
) -> Result<(Vec<Message>, Vec<i64>)> {
    let mut all_messages = messages.to_vec();
    let reply_context = load_reply_context(services, &all_messages).await?;
    all_messages.extend(reply_context);
    all_messages.sort_by(|a, b| a.active_at().cmp(&b.active_at()).then(a.id.cmp(&b.id)));
    let message_ids: Vec<i64> = all_messages.iter().map(|m| m.id).collect();
    Ok((all_messages, message_ids))
}

// ── Builder ─────────────────────────────────────────────────────────────────

/// 首轮全量上下文渲染（含感知、话题、记忆等完整信息）
pub async fn build_first_round_prompt(
    ctx: &RoundContext,
    messages: &[Message],
) -> Result<BuiltContext> {
    let services = &ctx.app.db.srv;
    let cfg = &ctx.app.cfg;
    let parser = ctx.bot.handler.content_parser();
    let chat_id = ctx.chat_id;

    let data = prepare_round_data(ctx, messages).await?;
    let search = search_related_context(
        services,
        cfg,
        chat_id,
        &data.topics,
        &data.parsed,
        &data.perception.items,
    )
    .await?;

    let shown_memory_ids: Vec<Uuid> = search.memories.iter().map(|m| m.id.0).collect();
    let shown_topic_ids: Vec<Uuid> = search.topics.iter().map(|t| t.topic.id).collect();

    let scratchpad = services.scratchpad.get(chat_id).await?.map(|s| s.content);
    let total_unread = services.message.count_unread_by_chat(chat_id).await?;

    let sandbox_info = Some(SandboxInfo::from(&cfg.sandbox));

    let context_data = RenderContextData {
        bot: ctx.bot.profile.clone(),
        chat: data.chat,
        current_time: jiff::Zoned::now().to_string(),
        messages: data.all_messages,
        total_unread: total_unread as i64,
        topics: data.topics,
        related_topics: search.topics,
        related_memories: search.memories,
        accounts: data.accounts,
        perceptions: data.perception.items,
        scratchpad,
        topic_idle_hours: cfg.agent.context.topic_idle_hours,
        sandbox_info,
    };
    let renderer = parser.create_renderer(&data.perception.map);
    let render_ctx = RenderContext::new(context_data, renderer);
    let rendered_prompt = render_main_context(&render_ctx, build_situation_section(&ctx.events));

    Ok(BuiltContext {
        rendered_prompt,
        message_ids: data.message_ids,
        shown_memory_ids,
        shown_topic_ids,
    })
}

/// 后续轮次增量上下文渲染（<new> 块）
pub async fn build_next_round_prompt(
    ctx: &RoundContext,
    messages: &[Message],
    shown_memory_ids: &HashSet<Uuid>,
    shown_topic_ids: &HashSet<Uuid>,
) -> Result<BuiltContext> {
    let services = &ctx.app.db.srv;
    let cfg = &ctx.app.cfg;
    let parser = ctx.bot.handler.content_parser();
    let chat_id = ctx.chat_id;

    if messages.is_empty() {
        return Ok(BuiltContext {
            rendered_prompt: String::new(),
            message_ids: vec![],
            shown_memory_ids: Vec::new(),
            shown_topic_ids: Vec::new(),
        });
    }

    let data = prepare_round_data(ctx, messages).await?;

    let perception_map = build_attachment_maps(services, parser, &data.all_messages).await?;
    let renderer = parser.create_renderer(&perception_map);
    let render_ctx = RenderContext::new(
        RenderContextData {
            bot: ctx.bot.profile.clone(),
            chat: data.chat,
            current_time: jiff::Zoned::now().to_string(),
            messages: data.all_messages.clone(),
            total_unread: 0,
            topics: vec![],
            related_topics: vec![],
            related_memories: vec![],
            accounts: data.accounts,
            perceptions: vec![],
            scratchpad: None,
            topic_idle_hours: cfg.agent.context.topic_idle_hours,
            sandbox_info: None,
        },
        renderer,
    );

    let msg_refs: Vec<&Message> = render_ctx.messages.iter().collect();
    let conversation = conversation_element(&msg_refs, &render_ctx);
    let chat_info = render_chat_info(&render_ctx.chat);

    let current_time = jiff::Zoned::now().to_string();

    let mut elements: Vec<Node> = Vec::new();

    // 1. <situation>
    let situation = build_situation_section(&ctx.events);
    if !situation.is_empty() {
        elements.push(situation);
    }

    // 2. <environment><current_time/>
    elements.push(
        Node::tag("environment").child(Node::tag("current_time").child(Node::text(current_time))),
    );

    // 3. <chat>
    elements.push(chat_info);

    // 4. <conversation>
    elements.push(conversation);

    // 5. 检索相关内容（排除已展示）
    let search = search_related_dedup(
        services,
        cfg,
        chat_id,
        &data.topics,
        &data.parsed,
        &data.perception.items,
        shown_memory_ids,
        shown_topic_ids,
    )
    .await?;

    let new_shown_memory_ids: Vec<Uuid> = search.memories.iter().map(|m| m.id.0).collect();
    let new_shown_topic_ids: Vec<Uuid> = search.topics.iter().map(|t| t.topic.id).collect();

    if !search.memories.is_empty() {
        elements.push(related_memories_section(
            &search.memories,
            "related_memories",
        ));
    }
    if !search.topics.is_empty() {
        let topic_nodes: Vec<Node> = search
            .topics
            .iter()
            .map(|r| topic_element_static(&r.topic).attr("relevance", format!("{:.4}", r.distance)))
            .collect();
        elements.push(Node::tag("related_topics").children(topic_nodes));
    }

    let new = render_pretty(Node::tag("new").children(elements), Format::Xml);

    Ok(BuiltContext {
        rendered_prompt: new,
        message_ids: data.message_ids,
        shown_memory_ids: new_shown_memory_ids,
        shown_topic_ids: new_shown_topic_ids,
    })
}
