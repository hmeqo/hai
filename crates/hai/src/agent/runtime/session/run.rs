//! Run 组装：合并 events、构建 prompt、派发 tokio task。

use std::sync::Arc;

use genai::chat::ChatMessage;
use tokio::{sync::oneshot, task::JoinHandle};

use super::{
    super::{
        context::RunContext,
        event::WakeEvent,
        types::{Run, RunOutput, RunPayload},
    },
    AgentSession, SessionState, proxy,
};
use crate::{
    agent::{
        context,
        node::main::build_react_config,
        runtime::{AgentEngine, react::run_react_loop},
        tools::get_main_agent_tools,
    },
    agentcore::tool::AgentTool,
    domain::{
        service::DbServices,
        vo::{ChatId, MessageId},
    },
};

impl AgentSession {
    pub(super) async fn assemble_run(
        &mut self,
        events: Vec<WakeEvent>,
    ) -> Option<(RunContext, RunPayload)> {
        let ctx = self.build_run_context(events);
        let (messages, next_since_id) = self.gather_messages().await;

        let payload = match &self.conversation {
            Some(conv) => conv.next_prompt(&ctx, &messages, next_since_id).await?,
            None => first_run_payload(&ctx, &messages, next_since_id).await?,
        };

        Some((ctx, payload))
    }

    pub(super) async fn on_run_complete(&mut self, run: Run, messages: Vec<ChatMessage>) {
        if run.sent_message() {
            self.schedule.refresh();
        }
        if let Some(conv) = &mut self.conversation {
            conv.push_run(run, messages);
        }
    }

    pub(super) fn build_status(&mut self) -> proxy::SessionStatus {
        let (run_in_progress, run_elapsed) = match &self.state {
            SessionState::Active(active) => (true, Some(active.started_at.elapsed().as_secs_f64())),
            SessionState::Idle => (false, None),
        };
        let runs_completed = self
            .conversation
            .as_ref()
            .map(|c| c.runs_completed())
            .unwrap_or(0);
        let last_run_turns = self
            .conversation
            .as_ref()
            .and_then(|c| c.last_run())
            .map(|r| r.turns.clone());
        proxy::SessionStatus {
            scheduler: self.schedule.snapshot(),
            runs_completed,
            run_in_progress,
            run_elapsed_secs: run_elapsed,
            model: self.engine.app.cfg.agent.model.clone(),
            last_run_turns,
        }
    }

    pub(super) fn answer_status(&mut self, query: oneshot::Sender<proxy::SessionStatus>) {
        let _ = query.send(self.build_status());
    }
}

// ── 首次 Run 的提示词构建（Ephemeral 模式或无 previous run）───────────────

async fn first_run_payload(
    ctx: &RunContext,
    messages: &[crate::domain::model::Message],
    next_since_id: i64,
) -> Option<RunPayload> {
    let built = context::build_first_run_prompt(ctx, messages).await.ok()?;

    Some(RunPayload {
        messages: built.messages,
        prompt: built.rendered_prompt,
        message_ids: built.message_ids,
        since_id: next_since_id,
        shown_memory_ids: built.shown_memory_ids,
        shown_topic_ids: built.shown_topic_ids,
    })
}

// ── Run 执行 ─────────────────────────────────────────────────────────────────

async fn collect_run_tools(ctx: &RunContext, engine: &AgentEngine) -> Vec<Arc<dyn AgentTool>> {
    let mut tools = get_main_agent_tools(&ctx.tool_ctx());
    tools.extend(engine.mcp_manager.list_all_tools().await);
    tools.extend(crate::agent::tools::skills::load_skill_tool(
        engine.skill_manager.clone(),
    ));
    tools
}

async fn mark_seen(chat_id: ChatId, msg_ids: &[i64], db: &DbServices) {
    if msg_ids.is_empty() {
        return;
    }
    let ids: Vec<MessageId> = msg_ids.iter().map(|id| MessageId(*id)).collect();
    if let Err(e) = db.message.mark_unread_seen(&ids).await {
        tracing::warn!(%chat_id, "Failed to mark messages seen: {e}");
    }
}

pub(super) fn spawn_run_task(
    engine: AgentEngine,
    ctx: RunContext,
    payload: RunPayload,
) -> (JoinHandle<()>, oneshot::Receiver<proxy::RunSignal>) {
    let bot = ctx.bot.clone();
    let chat_id = ctx.chat_id;
    let events_reasons: Vec<&str> = ctx.events.iter().map(|e| e.reason.label()).collect();

    let client = engine.client.clone();
    let model = engine.model.clone();
    let prompt_debug = payload.prompt.clone();
    let shown_memory_ids = payload.shown_memory_ids.clone();
    let shown_topic_ids = payload.shown_topic_ids.clone();

    let config = build_react_config(&engine, ctx.chat_type);

    let (tx, rx) = oneshot::channel();

    let handle = tokio::spawn(async move {
        let started_at = tokio::time::Instant::now();
        tracing::info!(%chat_id, reasons = ?events_reasons, "Agent woke up");
        tracing::debug!(%chat_id, "Run prompt:\n{}", prompt_debug);

        let _hb = proxy::HeartbeatTask::spawn(bot, chat_id);

        let all_tools = collect_run_tools(&ctx, &engine).await;

        let result = run_react_loop(client, &model, payload.messages, all_tools, &config).await;
        let elapsed = started_at.elapsed();

        match result {
            Ok(output) => {
                let tool_calls: usize = output.turns.iter().map(|t| t.tool_calls.len()).sum();
                tracing::info!(
                    %chat_id,
                    elapsed_secs = %elapsed.as_secs_f64(),
                    tool_calls,
                    final_response = %output.final_response,
                    "Agent done",
                );
                mark_seen(chat_id, &payload.message_ids, &ctx.db).await;

                let _ = tx.send(Some(RunOutput {
                    run: Run {
                        turns: output.turns,
                        since_id: payload.since_id,
                        shown_memory_ids,
                        shown_topic_ids,
                    },
                    messages: output.messages,
                }));
            }
            Err(e) => {
                tracing::error!(%chat_id, elapsed_secs = %elapsed.as_secs_f64(), "Agent run failed: {e}");
                let _ = tx.send(None);
            }
        }
    });

    (handle, rx)
}
