//! 事件派发：dispatch + on_complete + spawn_run

use std::sync::Arc;

use tokio::{sync::oneshot, task::JoinHandle};

use super::{
    super::{context::RunContext, event::WakeEvents},
    ActiveRun, AgentSession, Conversation, SessionState,
};
use crate::{
    agent::{
        runtime::{
            AgentEngine,
            event::Inbox,
            react::{ReactRun, run_react_loop},
            session::{
                prompt::RunInput,
                proxy::{HeartbeatTask, RunSignal},
            },
            types::RunOutput,
        },
        tools::get_main_agent_tools,
    },
    agentcore::tool::AgentTool,
    config::schema::ConversationMode,
    domain::{
        service::DbServices,
        vo::{AgentEventPayload, MessageId, TurnOutput},
    },
    error::Result,
};

// ── 生命周期 ───────────────────────────────────────────────────────────────

impl AgentSession {
    pub(super) async fn dispatch(&mut self, events: WakeEvents, inbox: &Inbox) {
        if !matches!(self.conversation.mode, ConversationMode::Persistent) {
            self.conversation = Conversation::new(ConversationMode::Ephemeral);
        }
        let Some((ctx, payload)) = self.assemble_run(events).await else {
            self.state = SessionState::Idle;
            return;
        };

        let run_number = self.conversation.next_run_number();
        let reason = ctx
            .events
            .first()
            .map(|e| e.reason.label())
            .unwrap_or("unknown")
            .to_string();

        self.engine.app.event_bus.emit(
            self.chat_id,
            AgentEventPayload::RunStarted {
                run: run_number,
                reason,
                msg_count: payload.message_ids.len(),
                full_prompt: payload.prompt.clone(),
            },
        );

        let (proc_handle, result_rx) =
            spawn_run(self.engine.clone(), ctx, payload, inbox.clone(), run_number);

        self.state = SessionState::Active(ActiveRun {
            handle: proc_handle,
            result_rx,
            started_at: tokio::time::Instant::now(),
        });
    }

    pub(super) async fn on_complete(&mut self, output: RunOutput, inbox: &Inbox) {
        if output.has_spoken {
            self.schedule.refresh();
        }
        self.conversation.update(&output);
        let events = inbox.drain();
        self.schedule.enqueue(events);
        self.state = SessionState::Idle;
    }
}

// ── Run Task ─────────────────────────────────────────────────────────────

async fn collect_run_tools(ctx: &RunContext, engine: &AgentEngine) -> Vec<Arc<dyn AgentTool>> {
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

pub(super) fn spawn_run(
    engine: AgentEngine,
    ctx: RunContext,
    payload: RunInput,
    inbox: Inbox,
    run_number: usize,
) -> (JoinHandle<()>, oneshot::Receiver<RunSignal>) {
    let handler = ctx.handler.clone();
    let chat_id = ctx.chat_id;
    let message_ids = payload.message_ids.clone();
    let payload_since_id = payload.since_id;
    let event_bus = engine.app.event_bus.clone();

    let run = ReactRun::new(&engine, &ctx, payload.messages, inbox, run_number);

    let (tx, rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        let started_at = tokio::time::Instant::now();

        let _hb = HeartbeatTask::spawn(handler, chat_id);

        let all_tools = collect_run_tools(&ctx, &engine).await;

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

                let last = output.turns.last().map(|t| TurnOutput {
                    run: run_number,
                    turn: output.turns.len(),
                    reasoning: t.reasoning.clone(),
                    response: t.response.clone(),
                });

                event_bus.emit(
                    chat_id,
                    AgentEventPayload::RunCompleted {
                        output: last.unwrap_or(TurnOutput {
                            run: run_number,
                            turn: 0,
                            reasoning: None,
                            response: String::new(),
                        }),
                        tool_calls,
                        elapsed_ms: elapsed.as_millis() as u64,
                        prompt_tokens: output.prompt_tokens,
                        completion_tokens: output.completion_tokens,
                        has_spoken,
                    },
                );

                let _ = mark_seen(&message_ids, &ctx.db).await;

                let _ = tx.send(Some(RunOutput {
                    messages: output.messages,
                    turns: output.turns,
                    prompt_tokens: output.prompt_tokens,
                    since_id: payload_since_id,
                    has_spoken,
                }));
            }
            Err(e) => {
                tracing::warn!(%chat_id, elapsed_secs = %elapsed.as_secs_f64(), error = %e, "Agent run failed");
                event_bus.emit(
                    chat_id,
                    AgentEventPayload::RunFailed {
                        run: run_number,
                        elapsed_ms: elapsed.as_millis() as u64,
                        error: e.to_string(),
                    },
                );
                let _ = tx.send(None);
            }
        }
    });

    (task, rx)
}
