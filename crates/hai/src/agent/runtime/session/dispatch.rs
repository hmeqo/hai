//! 事件派发：dispatch + on_complete + spawn_processing

use std::sync::Arc;

use tokio::{sync::oneshot, task::JoinHandle};

use super::{
    super::{context::RunContext, event::WakeEvent},
    ActiveProcessing, AgentSession, Conversation, SessionState,
};
use crate::{
    agent::{
        node::main::build_react_config,
        runtime::{
            AgentEngine, AgentEvent,
            react::{ReactRun, run_react_loop},
            types::{Inbox, ProcessingOutput},
        },
        tools::get_main_agent_tools,
    },
    agentcore::tool::AgentTool,
    config::schema::ConversationMode,
    domain::{service::DbServices, vo::MessageId},
    error::Result,
};

// ── 生命周期 ───────────────────────────────────────────────────────────────

impl AgentSession {
    pub(super) async fn dispatch(&mut self, events: Vec<WakeEvent>, inbox: &Inbox) {
        if !matches!(self.conversation.mode, ConversationMode::Persistent) {
            self.conversation = Conversation::new(ConversationMode::Ephemeral);
        }
        let Some((ctx, payload)) = self.assemble_run(events).await else {
            self.state = SessionState::Idle;
            return;
        };

        let turn = self.conversation.turn_count() + 1;
        let reason = ctx
            .events
            .first()
            .map(|e| e.reason.label())
            .unwrap_or("unknown")
            .to_string();

        self.engine.app.event_bus.emit(AgentEvent::WakeStarted {
            chat_id: self.chat_id,
            turn,
            reason,
        });
        self.engine.app.event_bus.emit(AgentEvent::ContextBuilt {
            chat_id: self.chat_id,
            turn,
            msg_count: payload.message_ids.len(),
            full_prompt: payload.prompt.clone(),
        });

        let (proc_handle, result_rx) =
            spawn_processing(self.engine.clone(), ctx, payload, inbox.clone(), turn);

        self.state = SessionState::Active(ActiveProcessing {
            handle: proc_handle,
            result_rx,
            started_at: tokio::time::Instant::now(),
        });
    }

    pub(super) async fn on_complete(&mut self, output: ProcessingOutput, inbox: &Inbox) {
        if output.has_spoken {
            self.schedule.refresh();
        }
        self.conversation.update(&output);
        let events = inbox.drain();
        self.schedule.enqueue(events);
        self.state = SessionState::Idle;
    }
}

// ── Processing Task ───────────────────────────────────────────────────────

async fn collect_processing_tools(
    ctx: &RunContext,
    engine: &AgentEngine,
) -> Vec<Arc<dyn AgentTool>> {
    let mut tools = get_main_agent_tools(&ctx.tool_ctx());
    tools.extend(engine.mcp_manager.list_all_tools().await);
    tools
}

async fn mark_seen(msg_ids: &[MessageId], db: &DbServices) -> Result<()> {
    if msg_ids.is_empty() {
        return Ok(());
    }
    db.message.mark_unread_seen(msg_ids).await?;
    Ok(())
}

pub(super) fn spawn_processing(
    engine: AgentEngine,
    ctx: RunContext,
    payload: super::prompt::ProcessingPayload,
    inbox: Inbox,
    outer_turn: usize,
) -> (
    JoinHandle<()>,
    oneshot::Receiver<super::proxy::ProcessingSignal>,
) {
    let bot = ctx.bot.clone();
    let chat_id = ctx.chat_id;
    let config = build_react_config(&engine, ctx.chat_type);
    let message_ids = payload.message_ids.clone();
    let payload_since_id = payload.since_id;
    let payload_messages = payload.messages;
    let event_bus = engine.app.event_bus.clone();

    let run = ReactRun {
        client: engine.client.clone(),
        model: engine.model.clone(),
        messages: payload_messages,
        config,
        inbox,
        preempt: engine.app.cfg.agent.context.preempt,
        event_bus: event_bus.clone(),
        chat_id,
        outer_turn,
    };

    let (tx, rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        let started_at = tokio::time::Instant::now();

        let _hb = super::proxy::HeartbeatTask::spawn(bot, chat_id);

        let all_tools = collect_processing_tools(&ctx, &engine).await;

        let result = run_react_loop(run, all_tools).await;

        let elapsed = started_at.elapsed();

        match result {
            Ok(output) => {
                let tool_calls: usize = output.turns.iter().map(|t| t.tool_calls.len()).sum();
                let has_spoken = output
                    .turns
                    .iter()
                    .flat_map(|t| &t.tool_calls)
                    .any(|tc| matches!(tc.tool_name.as_str(), "send_message" | "send_voice"));

                let (response, reasoning) = output
                    .turns
                    .last()
                    .map(|t| (t.response.clone(), t.reasoning.clone()))
                    .unwrap_or_default();

                event_bus.emit(AgentEvent::RunCompleted {
                    chat_id,
                    turn: outer_turn,
                    tool_calls,
                    elapsed_ms: elapsed.as_millis() as u64,
                    prompt_tokens: output.prompt_tokens,
                    completion_tokens: output.completion_tokens,
                    has_spoken,
                    response,
                    reasoning,
                });

                let _ = mark_seen(&message_ids, &ctx.db).await;

                let _ = tx.send(Some(ProcessingOutput {
                    messages: output.messages,
                    turns: output.turns,
                    prompt_tokens: output.prompt_tokens,
                    since_id: payload_since_id,
                    has_spoken,
                }));
            }
            Err(e) => {
                tracing::warn!(%chat_id, elapsed_secs = %elapsed.as_secs_f64(), error = %e, "Agent run failed");
                event_bus.emit(AgentEvent::RunFailed {
                    chat_id,
                    turn: outer_turn,
                    elapsed_ms: elapsed.as_millis() as u64,
                    error: e.to_string(),
                });
                let _ = tx.send(None);
            }
        }
    });

    (task, rx)
}
