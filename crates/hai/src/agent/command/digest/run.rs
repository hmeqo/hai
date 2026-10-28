//! The digest pass: one read-only run over a stretch of the chat's own record. It writes no chat
//! data — no entry, no message row — traces its steps under its own session, and hands back the
//! report it wrote.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use genai::chat::ChatMessage;

use super::{copy, prompt};
use crate::{
    agent::{
        channel::ChannelHandle,
        process::AgentRuntime,
        prompt::{
            context::{
                ContextMessages, RenderContext,
                load::{collect_accounts, load_chat},
                render_context::RenderContextData,
                render_context_for_digest,
            },
            role::AgentRole,
        },
        run::{
            Messages, SelectedModel,
            record::SessionRecord,
            step_loop::{StepConfig, StepLoop},
        },
    },
    agentcore::{
        render::Node,
        tool::{AgentTool, ToolCallContext},
    },
    config::conversation::{ContentPolicy, Language},
    domain::{
        model::ChatType,
        vo::{AgentEvent, AgentEventPayload, ChatId, SessionId, SessionRef, Stretch, TurnNumber},
    },
};

/// What one pass leaves: the report's text, the steps it spent, and whether the loop was cut off
/// before the model said it was finished.
pub(crate) struct DigestReport {
    pub text: String,
    pub steps: usize,
    pub truncated: bool,
}

/// One digest pass's inputs: the chat whose record it reads, the language of the report, and the
/// services behind it.
pub(crate) struct DigestPass<'a> {
    pub runtime: &'a AgentRuntime,
    pub chat_id: ChatId,
    pub chat_type: ChatType,
    pub language: Language,
    pub model: &'a SelectedModel,
    pub content_policy: ContentPolicy,
    pub handle: &'a ChannelHandle,
    pub stretch: Stretch,
    pub tools: &'a [Arc<dyn AgentTool>],
}

/// One digest: the report it wrote, or the failure's own words. The pass is its own session, so its
/// steps are traced even though it writes no chat data.
pub(crate) async fn run(pass: DigestPass<'_>) -> Result<DigestReport, String> {
    let session = SessionRef::digest(SessionId::new());
    let chat_id = pass.chat_id;
    let bus = pass.runtime.event_bus().clone();
    bus.emit(AgentEvent {
        chat_id,
        session: Some(session),
        payload: AgentEventPayload::DigestStarted {
            stretch: pass.stretch,
            language: pass.language,
        },
    });

    let result = run_inner(pass, session).await;

    let payload = match &result {
        Ok(report) => AgentEventPayload::DigestCompleted {
            steps: report.steps,
            truncated: report.truncated,
            report_chars: report.text.chars().count(),
        },
        Err(error) => AgentEventPayload::DigestFailed {
            error: error.clone(),
        },
    };
    bus.emit(AgentEvent {
        chat_id,
        session: Some(session),
        payload,
    });
    result
}

async fn run_inner(pass: DigestPass<'_>, session: SessionRef) -> Result<DigestReport, String> {
    let DigestPass {
        runtime,
        chat_id,
        chat_type,
        language,
        model,
        content_policy,
        handle,
        stretch,
        tools,
    } = pass;
    let services = runtime.services();
    let cfg = runtime.cfg();

    // The window's rows are read for one thing: who spoke in it. The report is written from what the
    // pass goes and finds with its own tools, never from a message list handed to it.
    let window = services
        .message
        .in_range(chat_id, stretch.resolve(jiff::Timestamp::now()))
        .await
        .map_err(|e| e.to_string())?;
    let accounts = collect_accounts(services, &window, &HashMap::new())
        .await
        .map_err(|e| e.to_string())?;
    let chat = load_chat(services, chat_id)
        .await
        .map_err(|e| e.to_string())?;
    let conversation_id = chat.conversation_id;
    let chats = services
        .platform
        .chats_in_conversation(conversation_id)
        .await
        .map_err(|e| e.to_string())?;
    let roster: Arc<[_]> = chats.iter().map(|chat| chat.roster()).collect();
    let data = RenderContextData {
        channel: handle.profile().clone(),
        chat: Some(chat.roster()),
        chats: roster,
        chat_aliases: Default::default(),
        origin_chat: chat_id,
        roster_dirty: false,
        host_actions: Vec::new(),
        language,
        current_time: jiff::Zoned::now().to_string(),
        first_turn: true,
        messages: ContextMessages::new(Vec::new(), HashMap::new()),
        new_message_ids: HashSet::new(),
        attachments_by_message: HashMap::new(),
        perception_by_attachment: HashMap::new(),
        same_resource_as: HashMap::new(),
        accounts,
        current_topics: services
            .topic
            .active_topics(conversation_id)
            .await
            .map_err(|e| e.to_string())?,
        pinned_memories: services
            .memory
            .pinned(conversation_id, cfg.agent.context.pinned_memory_limit)
            .await
            .map_err(|e| e.to_string())?,
        recalled_memories: Vec::new(),
        recalled_topics: Vec::new(),
        perceptions: Vec::new(),
        topic_idle_hours: cfg.agent.context.topic_idle_hours,
    };

    let render_ctx = RenderContext::new(data);
    let instruction = Node::tag("situation").child(
        Node::tag("trigger")
            .attr("reason", "command")
            .child(Node::text(prompt::task(language, stretch))),
    );
    let rendered = render_context_for_digest(&render_ctx, instruction);

    let spec = AgentRole::Digest.spec(
        runtime,
        model.chat_options(),
        chat_type,
        language,
        None,
        content_policy,
    );
    let step_loop = StepLoop::new(
        model.client.clone(),
        model.model.clone(),
        ToolCallContext {
            image_input: model.capabilities.image_input,
        },
        Messages::new(vec![ChatMessage::user(&rendered)]),
        spec,
        StepConfig {
            // The pass reads before it writes, so it spends `[agent] max-steps` like a Turn does.
            budget: cfg.agent.max_steps,
            steering: None,
        },
        SessionRecord {
            event_bus: runtime.event_bus().clone(),
            chat_id,
            session,
            turn: TurnNumber::from(1),
        },
    );
    let output = step_loop.run(tools).await.map_err(|e| e.to_string())?;

    let report = output
        .steps
        .iter()
        .rev()
        .find(|step| !step.response.trim().is_empty())
        .map(|step| step.response.clone());
    match report {
        Some(text) => Ok(DigestReport {
            text,
            steps: output.steps.len(),
            truncated: output.truncated,
        }),
        // A run that spent its steps reading has unfinished work, not an empty report.
        None if output.truncated => Err(copy::truncated(language)),
        None => Err(copy::empty(language)),
    }
}
