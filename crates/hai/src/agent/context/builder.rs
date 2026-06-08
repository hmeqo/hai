use crate::{
    agent::{
        context::{
            RenderContext, build_last_round_section, build_situation_section,
            helper::{
                build_attachment_maps, collect_accounts, load_chat, load_perceptions,
                load_reply_context, search_related_context,
            },
            render_context::{RenderContextData, SandboxInfo},
            render_main_context,
            sections::message::messages_elements,
        },
        link::BuiltContext,
        runtime::{ctx::RoundCtx, round::Round},
    },
    agentcore::render::{Format, Node, render_pretty},
    domain::entity::Message,
    error::Result,
};

/// 首轮全量上下文渲染（含感知、话题、记忆等完整信息）
pub async fn build_first_round_prompt(
    ctx: &RoundCtx,
    messages: &[Message],
) -> Result<BuiltContext> {
    let services = &ctx.app.db.srv;
    let cfg = &ctx.app.cfg;
    let parser = ctx.bot.handler.content_parser();
    let chat_id = ctx.chat_id;

    let mut all_messages = messages.to_vec();
    let reply_context = load_reply_context(services, &all_messages).await?;
    all_messages.extend(reply_context);
    all_messages.sort_by(|a, b| {
        a.active_at_sqlx()
            .cmp(&b.active_at_sqlx())
            .then(a.id.cmp(&b.id))
    });
    let message_ids: Vec<i64> = all_messages.iter().map(|m| m.id).collect();

    let parsed: Vec<_> = all_messages
        .iter()
        .map(|m| parser.parse(&m.content))
        .collect();
    let perception = load_perceptions(services, &parsed).await?;

    let topics = services.topic.get_active_topics(chat_id).await?;
    let search =
        search_related_context(services, cfg, chat_id, &topics, &parsed, &perception.items).await?;
    let accounts = collect_accounts(services, &all_messages).await?;
    let scratchpad = services.scratchpad.get(chat_id).await?.map(|s| s.content);
    let total_unread = services.message.count_unread_by_chat(chat_id).await?;
    let chat = load_chat(services, chat_id).await?;

    let sandbox_info = Some(SandboxInfo::from(&cfg.sandbox));

    let data = RenderContextData {
        bot: ctx.bot.profile.clone(),
        chat,
        current_time: jiff::Zoned::now().to_string(),
        messages: all_messages,
        total_unread,
        topics,
        related_topics: search.topics,
        related_memories: search.memories,
        accounts,
        perceptions: perception.items,
        scratchpad,
        topic_idle_hours: cfg.agent.context.topic_idle_hours,
        sandbox_info,
    };
    let renderer = parser.create_renderer(&perception.map);
    let render_ctx = RenderContext::new(data, renderer);
    let rendered_prompt = render_main_context(&render_ctx, build_situation_section(&ctx.events));

    Ok(BuiltContext {
        rendered_prompt,
        message_ids,
    })
}

/// 后续轮次增量上下文渲染（last-round + 新消息）
pub async fn build_next_round_prompt(
    ctx: &RoundCtx,
    messages: &[Message],
    prev_round: Option<&Round>,
) -> Result<BuiltContext> {
    let services = &ctx.app.db.srv;
    let cfg = &ctx.app.cfg;
    let parser = ctx.bot.handler.content_parser();
    let chat_id = ctx.chat_id;

    if messages.is_empty() {
        return Ok(BuiltContext {
            rendered_prompt: String::new(),
            message_ids: vec![],
        });
    }

    let mut all_messages = messages.to_vec();
    let reply_context = load_reply_context(services, &all_messages).await?;
    all_messages.extend(reply_context);
    all_messages.sort_by_key(|a| a.id);

    let message_ids: Vec<i64> = messages.iter().map(|m| m.id).collect();
    let perception_map = build_attachment_maps(services, parser, &all_messages).await?;
    let renderer = parser.create_renderer(&perception_map);
    let accounts = collect_accounts(services, &all_messages).await?;
    let chat = load_chat(services, chat_id).await?;

    let data = RenderContextData {
        bot: ctx.bot.profile.clone(),
        chat,
        current_time: jiff::Zoned::now().to_string(),
        messages: all_messages,
        total_unread: 0,
        topics: vec![],
        related_topics: vec![],
        related_memories: vec![],
        accounts,
        perceptions: vec![],
        scratchpad: None,
        topic_idle_hours: cfg.agent.context.topic_idle_hours,
        sandbox_info: None,
    };
    let render_ctx = RenderContext::new(data, renderer);

    let msg_refs: Vec<&Message> = messages.iter().collect();
    let message_elements = messages_elements(&msg_refs, &render_ctx);

    let mut elements: Vec<Node> = Vec::new();
    if let Some(prev) = prev_round
        && let Some(section) = build_last_round_section(prev)
    {
        elements.push(section);
    }
    elements.push(Node::tag("current_time").child(Node::text(jiff::Zoned::now().to_string())));
    let situation = build_situation_section(&ctx.events);
    if !situation.is_empty() {
        elements.push(situation);
    }
    elements.push(Node::tag("messages").children(message_elements));

    let update = render_pretty(Node::tag("update").children(elements), Format::Xml);

    Ok(BuiltContext {
        rendered_prompt: update,
        message_ids,
    })
}
