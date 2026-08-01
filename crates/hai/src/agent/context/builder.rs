use std::collections::HashSet;

use genai::chat::ChatMessage;
use uuid::Uuid;

use crate::{
    agent::{
        context::{
            RenderContext, build_situation_section,
            helper::{
                SearchRelatedParams, build_attachment_maps, collect_accounts, load_chat,
                load_perceptions, load_reply_context, search_related_context, search_related_dedup,
            },
            render_context,
            render_context::RenderContextData,
            render_main_context,
        },
        link::BuiltContext,
        runtime::context::RunContext,
    },
    domain::model::Message,
    error::Result,
};

// ── Shared data ──────────────────────────────────────────────────────────────

struct RunData {
    all_messages: Vec<Message>,
    message_ids: Vec<i64>,
    parsed: Vec<crate::agent::context::types::ParsedContent>,
    perception: crate::agent::context::types::PerceptionResult,
    topics: Vec<crate::domain::model::Topic>,
    accounts: Vec<crate::domain::model::Account>,
    chat: crate::domain::model::Chat,
}

async fn prepare_run_data(ctx: &RunContext, messages: &[Message]) -> Result<RunData> {
    let services = &ctx.app.db.srv;
    let parser = ctx.handler.content_parser();
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

    Ok(RunData {
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

/// 构建上下文 prompt。首轮全量渲染，后续轮次增量。
pub async fn build_prompt(
    ctx: &RunContext,
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

    let data = prepare_run_data(ctx, messages).await?;

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

    let total_unread = if is_first {
        services.message.count_unread_by_chat(chat_id).await? as i64
    } else {
        0
    };

    let perception_map = if is_first {
        data.perception.map
    } else {
        build_attachment_maps(services, parser, &data.all_messages).await?
    };
    let renderer = parser.create_renderer(&perception_map);

    let context_data = RenderContextData {
        bot: ctx.handler.profile(),
        chat: data.chat,
        current_time: jiff::Zoned::now().to_string(),
        messages: data.all_messages,
        total_unread,
        topics: if is_first { data.topics } else { vec![] },
        related_topics: search.topics,
        related_memories: search.memories,
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

    let rendered = if is_first {
        render_main_context(&render_ctx, instruction)
    } else {
        render_context(&render_ctx, instruction, "new")
    };

    Ok(BuiltContext {
        messages: vec![ChatMessage::user(&rendered)],
        rendered_prompt: rendered,
        message_ids: data.message_ids,
        shown_memory_ids: new_shown_memory_ids,
        shown_topic_ids: new_shown_topic_ids,
    })
}
