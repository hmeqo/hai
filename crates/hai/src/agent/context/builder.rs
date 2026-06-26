use crate::{
    agent::{
        context::{
            RenderContext, build_situation_section,
            helper::{
                build_attachment_maps, collect_accounts, load_chat, load_perceptions,
                load_reply_context, search_related_context,
            },
            render_context::{RenderContextData, SandboxInfo},
            render_main_context,
            sections::{chat::render_chat_info, message::conversation_element},
        },
        link::BuiltContext,
        runtime::ctx::RoundContext,
    },
    agentcore::render::{Format, Node, render_pretty},
    domain::entity::Message,
    error::Result,
};

/// 加载 reply context → 合并 → 排序 → 提取 message_ids
async fn prepare_messages(
    services: &crate::domain::service::DbServices,
    messages: &[Message],
) -> Result<(Vec<Message>, Vec<i64>)> {
    let mut all_messages = messages.to_vec();
    let reply_context = load_reply_context(services, &all_messages).await?;
    all_messages.extend(reply_context);
    all_messages.sort_by(|a, b| {
        a.active_at_sqlx()
            .cmp(&b.active_at_sqlx())
            .then(a.id.cmp(&b.id))
    });
    let message_ids: Vec<i64> = all_messages.iter().map(|m| m.id).collect();
    Ok((all_messages, message_ids))
}

/// 首轮全量上下文渲染（含感知、话题、记忆等完整信息）
pub async fn build_first_round_prompt(
    ctx: &RoundContext,
    messages: &[Message],
) -> Result<BuiltContext> {
    let services = &ctx.app.db.srv;
    let cfg = &ctx.app.cfg;
    let parser = ctx.bot.handler.content_parser();
    let chat_id = ctx.chat_id;

    let (all_messages, message_ids) = prepare_messages(services, messages).await?;

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

/// 后续轮次增量上下文渲染（<new> 块）
pub async fn build_next_round_prompt(
    ctx: &RoundContext,
    messages: &[Message],
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

    let (all_messages, message_ids) = prepare_messages(services, messages).await?;
    let perception_map = build_attachment_maps(services, parser, &all_messages).await?;
    let renderer = parser.create_renderer(&perception_map);
    let accounts = collect_accounts(services, &all_messages).await?;
    let chat = load_chat(services, chat_id).await?;
    let chat_info = render_chat_info(&chat);

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

    let msg_refs: Vec<&Message> = render_ctx.messages.iter().collect();
    let conversation = conversation_element(&msg_refs, &render_ctx);

    let current_time = jiff::Zoned::now().to_string();

    let mut elements: Vec<Node> = Vec::new();

    // 1. <situation> — 和 <context> 对齐，在第一子
    let situation = build_situation_section(&ctx.events);
    if !situation.is_empty() {
        elements.push(situation);
    }

    // 2. <environment><current_time/></environment>
    elements.push(
        Node::tag("environment").child(Node::tag("current_time").child(Node::text(current_time))),
    );

    // 3. <chat> — 标识当前聊天
    elements.push(chat_info);

    // 4. <conversation> — 同首轮完全一致的消息渲染
    elements.push(conversation);

    let new = render_pretty(Node::tag("new").children(elements), Format::Xml);

    Ok(BuiltContext {
        rendered_prompt: new,
        message_ids,
    })
}
