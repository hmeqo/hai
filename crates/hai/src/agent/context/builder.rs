use std::collections::HashSet;

use genai::chat::ChatMessage;
use uuid::Uuid;

use crate::{
    agent::{
        context::{
            ContextMessages, RenderContext, build_situation_section,
            helper::{
                SearchRelatedParams, build_search_query, collect_accounts, load_chat,
                load_perceptions, load_reply_map, search_related_context, search_related_dedup,
            },
            render_context::RenderContextData,
            render_main_context,
        },
        link::BuiltContext,
        runtime::context::TurnContext,
    },
    domain::model::Message,
    error::Result,
};

// ── Shared data ──────────────────────────────────────────────────────────────

struct PromptInput {
    /// 对话流（窗口）+ 窗口外引用上下文。
    messages: ContextMessages,
    /// 窗口消息 id（mark_seen 用）
    message_ids: Vec<i64>,
    /// 消息解析（RAG 检索用）
    parsed: Vec<crate::agent::context::types::ParsedContent>,
    /// 附件感知（items 注入感知节 + map 供渲染器内联分析）
    perception: crate::agent::context::types::PerceptionResult,
    topics: Vec<crate::domain::model::Topic>,
    accounts: Vec<crate::domain::model::Account>,
    chat: crate::domain::model::Chat,
}

/// 装配 build_prompt 所需的全部输入：一次组织 DB 查询（消息/引用/感知/话题/账号/chat），
/// 供 RAG 检索与上下文渲染共用。
async fn prepare_prompt_input(ctx: &TurnContext, messages: &[Message]) -> Result<PromptInput> {
    let services = &ctx.app.db.srv;
    let parser = ctx.handler.content_parser();
    let chat_id = ctx.chat_id;

    // 窗口消息即对话流；窗口外引用映射单独构建（不污染对话流渲染）
    let reply_map = load_reply_map(services, messages).await?;
    let parsed: Vec<_> = messages.iter().map(|m| parser.parse(&m.content)).collect();
    let perception = load_perceptions(services, &parsed).await?;
    // 首轮全量注入上限：活跃话题截断（防 prompt 膨胀；配置 related_topic_limit）
    let mut topics = services.topic.get_active_topics(chat_id).await?;
    let inject_limit = ctx.app.cfg.agent.context.related_topic_limit as usize;
    if topics.len() > inject_limit {
        topics.truncate(inject_limit);
    }
    let accounts = collect_accounts(services, messages, &reply_map).await?;
    let chat = load_chat(services, chat_id).await?;

    Ok(PromptInput {
        message_ids: messages.iter().map(|m| m.id).collect(),
        messages: ContextMessages::new(messages.to_vec(), reply_map),
        parsed,
        perception,
        topics,
        accounts,
        chat,
    })
}

// ── Builder ─────────────────────────────────────────────────────────────────

/// 构建上下文 prompt。首轮全量渲染，后续轮次增量。
pub async fn build_prompt(
    ctx: &TurnContext,
    messages: &[Message],
    shown_memory_ids: &HashSet<Uuid>,
    shown_topic_ids: &HashSet<Uuid>,
    is_first: bool,
) -> Result<BuiltContext> {
    let services = &ctx.app.db.srv;
    let cfg = &ctx.app.cfg;
    let parser = ctx.handler.content_parser();
    let chat_id = ctx.chat_id;

    if !is_first && messages.is_empty() {
        return Ok(BuiltContext {
            rendered_prompt: String::new(),
            messages: vec![],
            message_ids: vec![],
            shown_memory_ids: Vec::new(),
            shown_topic_ids: Vec::new(),
        });
    }

    let data = prepare_prompt_input(ctx, messages).await?;

    let (search, new_shown_memory_ids, new_shown_topic_ids) = if is_first {
        let search = search_related_context(SearchRelatedParams {
            services,
            cfg,
            chat_id,
            topics: &data.topics,
            parsed: &data.parsed,
            perceptions: &data.perception.items,
            shown_memory_ids: &HashSet::new(),
            shown_topic_ids: &HashSet::new(),
        })
        .await?;
        let memory_ids: Vec<Uuid> = search.memories.iter().map(|m| m.id.0).collect();
        let topic_ids: Vec<Uuid> = search.topics.iter().map(|t| t.topic.id).collect();
        (Some(search), memory_ids, topic_ids)
    } else {
        let search = search_related_dedup(SearchRelatedParams {
            services,
            cfg,
            chat_id,
            topics: &data.topics,
            parsed: &data.parsed,
            perceptions: &data.perception.items,
            shown_memory_ids,
            shown_topic_ids,
        })
        .await?;
        let memory_ids: Vec<Uuid> = search.memories.iter().map(|m| m.id.0).collect();
        let topic_ids: Vec<Uuid> = search.topics.iter().map(|t| t.topic.id).collect();
        (Some(search), memory_ids, topic_ids)
    };
    let search = search.unwrap();

    let related_knowledge = if is_first && cfg.knowledge.inject.enable {
        let inject = &cfg.knowledge.inject;
        let query = build_search_query(&data.topics, &data.parsed, &data.perception.items);
        if query.is_empty() {
            Vec::new()
        } else {
            services
                .knowledge
                .search(&query, inject.limit, &inject.collections)
                .await?
        }
    } else {
        Vec::new()
    };

    let total_unread = if is_first {
        services.message.count_unread_by_chat(chat_id).await? as i64
    } else {
        0
    };

    // 首轮/后续轮同源：prepare 已查附件感知（load_perceptions），渲染器直接复用（不重复查询）
    let renderer = parser.create_renderer(&data.perception.map);

    let context_data = RenderContextData {
        bot: ctx.handler.profile(),
        chat: data.chat,
        current_time: jiff::Zoned::now().to_string(),
        messages: data.messages,
        total_unread,
        topics: if is_first { data.topics } else { vec![] },
        related_topics: search.topics,
        related_memories: search.memories,
        related_knowledge,
        accounts: data.accounts,
        perceptions: if is_first {
            data.perception.items
        } else {
            vec![]
        },
        topic_idle_hours: cfg.agent.context.topic_idle_hours,
    };
    let render_ctx = RenderContext::new(context_data, renderer);
    let instruction = build_situation_section(&ctx.events.coalesce());

    let rendered = render_main_context(&render_ctx, instruction);

    Ok(BuiltContext {
        messages: vec![ChatMessage::user(&rendered)],
        rendered_prompt: rendered,
        message_ids: data.message_ids,
        shown_memory_ids: new_shown_memory_ids,
        shown_topic_ids: new_shown_topic_ids,
    })
}
