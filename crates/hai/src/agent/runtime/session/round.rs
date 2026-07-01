//! 轮次组装：合并 events、构建 prompt、派发 tokio task。
//!
//! `assemble_round` 将一批 WakeEvent 合并为执行上下文 + 消息列表，
//! `spawn_round_task` 将之派发为独立 tokio task（通过 oneshot 返回结果）。

use std::sync::Arc;

use tokio::{sync::oneshot, task::JoinHandle};
use uuid::Uuid;

use super::{
    super::{
        ctx::RoundContext,
        event::WakeEvent,
        round::{Round, RoundTaskPayload},
    },
    SessionLoop, proxy,
};
use crate::{
    agent::{context, node::AgentNode, tools::get_main_agent_tools},
    agentcore::tool::AgentTool,
    config::schema::SessionConfig,
    domain::vo::MessageId,
};

impl SessionLoop {
    pub(super) async fn assemble_round(
        &mut self,
        events: Vec<WakeEvent>,
    ) -> Option<(RoundContext, RoundTaskPayload)> {
        let ctx = self.build_round_context(events);
        let (messages, next_since_id) = self.gather_messages().await;

        let built = if self.rounds.last().is_some() {
            let shown_memory_ids: std::collections::HashSet<Uuid> = self
                .rounds
                .iter()
                .flat_map(|r| r.shown_memory_ids.iter().copied())
                .collect();
            let shown_topic_ids: std::collections::HashSet<Uuid> = self
                .rounds
                .iter()
                .flat_map(|r| r.shown_topic_ids.iter().copied())
                .collect();
            match context::build_next_round_prompt(
                &ctx,
                &messages,
                &shown_memory_ids,
                &shown_topic_ids,
            )
            .await
            {
                Ok(b) => b,
                Err(e) => {
                    tracing::error!(%self.chat_id, "build_next_round_prompt failed: {e}");
                    return None;
                }
            }
        } else {
            match context::build_first_round_prompt(&ctx, &messages).await {
                Ok(b) => b,
                Err(e) => {
                    tracing::error!(%self.chat_id, "build_first_round_prompt failed: {e}");
                    return None;
                }
            }
        };

        let is_continuous = self.engine.app.cfg.agent.context.session == SessionConfig::Continuous;

        let full_messages = if !self.rounds.is_empty() && is_continuous {
            let mut msgs = self.messages.clone();
            msgs.extend(built.messages);
            msgs
        } else {
            built.messages
        };

        Some((
            ctx,
            RoundTaskPayload {
                messages: full_messages,
                prompt: built.rendered_prompt,
                message_ids: built.message_ids,
                since_id: next_since_id,
                shown_memory_ids: built.shown_memory_ids,
                shown_topic_ids: built.shown_topic_ids,
            },
        ))
    }

    pub(super) async fn on_round_complete(&mut self, output: Round) {
        if output.sent_message() {
            self.schedule.refresh();
        }
        self.messages = output.messages.clone();
        self.rounds.push(output);
        if matches!(
            self.engine.app.cfg.agent.context.session,
            SessionConfig::SingleRound
        ) {
            self.rounds.clear();
            self.messages.clear();
        }
    }

    pub(super) fn build_status(
        &mut self,
        round_running: bool,
        round_started: Option<tokio::time::Instant>,
    ) -> proxy::SessionStatus {
        proxy::SessionStatus {
            scheduler: self.schedule.snapshot(),
            rounds_completed: self.rounds.len(),
            round_running,
            round_elapsed_secs: round_started.map(|t| t.elapsed().as_secs_f64()),
            model: self.engine.app.cfg.agent.model.clone(),
        }
    }

    pub(super) fn answer_status(
        &mut self,
        query: oneshot::Sender<proxy::SessionStatus>,
        round_running: bool,
        round_started: Option<tokio::time::Instant>,
    ) {
        let _ = query.send(self.build_status(round_running, round_started));
    }
}

// ── Round 执行 ─────────────────────────────────────────────────────────────────

async fn collect_round_tools(ctx: &RoundContext, engine: &AgentEngine) -> Vec<Arc<dyn AgentTool>> {
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

use crate::{
    agent::runtime::AgentEngine,
    domain::{service::DbServices, vo::ChatId},
};

pub(super) fn spawn_round_task(
    engine: AgentEngine,
    ctx: RoundContext,
    payload: RoundTaskPayload,
) -> (JoinHandle<()>, oneshot::Receiver<proxy::RoundSignal>) {
    let bot = ctx.bot.clone();
    let chat_id = ctx.chat_id;
    let events_reasons: Vec<&str> = ctx.events.iter().map(|e| e.reason.label()).collect();

    let prompt_debug = payload.prompt.clone();
    let shown_memory_ids = payload.shown_memory_ids.clone();
    let shown_topic_ids = payload.shown_topic_ids.clone();

    let node = engine.build_node(ctx.chat_type);

    let (tx, rx) = oneshot::channel();

    let handle = tokio::spawn(async move {
        let started_at = tokio::time::Instant::now();
        tracing::info!(%chat_id, reasons = ?events_reasons, "Agent woke up");
        tracing::debug!(%chat_id, "Round prompt:\n{}", prompt_debug);

        let _hb = proxy::HeartbeatTask::spawn(bot, chat_id);

        let all_tools = collect_round_tools(&ctx, &engine).await;

        let result = node.run(payload.messages, all_tools).await;
        let elapsed = started_at.elapsed();

        match result {
            Ok(output) => {
                let tool_calls = output.tool_calls.len();
                tracing::info!(
                    %chat_id,
                    elapsed_secs = %elapsed.as_secs_f64(),
                    tool_calls,
                    final_response = %output.final_response,
                    "Agent done",
                );
                mark_seen(chat_id, &payload.message_ids, &ctx.db).await;

                let _ = tx.send(Some(Round {
                    messages: output.messages,
                    tool_calls: output.tool_calls,
                    since_id: payload.since_id,
                    shown_memory_ids,
                    shown_topic_ids,
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
