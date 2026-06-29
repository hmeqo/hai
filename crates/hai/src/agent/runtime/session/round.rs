//! 轮次组装：合并 events、构建 prompt、派发 tokio task。
//!
//! `assemble_round` 将一批 WakeEvent 合并为执行上下文 + prompt，
//! `spawn_round_task` 将之派发为独立 tokio task（通过 oneshot 返回结果）。

use std::collections::HashSet;

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
    agent::context,
    agentcore::render::{Format, render_pretty},
    config::schema::SessionConfig,
};

impl SessionLoop {
    pub(super) async fn assemble_round(
        &self,
        events: Vec<WakeEvent>,
    ) -> Option<(RoundContext, RoundTaskPayload)> {
        let ctx = self.build_round_context(events);
        let (messages, next_since_id) = self.gather_messages().await;

        let built = if self.rounds.last().is_some() {
            // 收集所有历史轮次已展示的 ID
            let shown_memory_ids: HashSet<Uuid> = self
                .rounds
                .iter()
                .flat_map(|r| r.shown_memory_ids.iter().copied())
                .collect();
            let shown_topic_ids: HashSet<Uuid> = self
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

        let mut segment = built.rendered_prompt;
        let is_continuous = self.engine.app.cfg.agent.context.session == SessionConfig::Continuous;

        if is_continuous
            && let Some(prev_round) = self.rounds.last()
            && let Some(round_end) = context::build_round_end_section(prev_round)
        {
            segment = format!(
                "{round_end_xml}\n{segment}",
                round_end_xml = render_pretty(round_end, Format::Xml)
            );
        }

        let prompt = if !self.rounds.is_empty() {
            format!("{}\n{segment}", self.full_prompt())
        } else {
            segment.clone()
        };

        Some((
            ctx,
            RoundTaskPayload {
                prompt,
                segment,
                message_ids: built.message_ids,
                since_id: next_since_id,
                shown_memory_ids: built.shown_memory_ids,
                shown_topic_ids: built.shown_topic_ids,
            },
        ))
    }

    fn full_prompt(&self) -> String {
        self.rounds
            .iter()
            .map(|r| r.segment.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// 轮次完成：记录历史、刷新调度器、判断是否 SingleRound 下清理。
    pub(super) async fn on_round_complete(&mut self, output: Round) {
        let did_send = output
            .tool_calls
            .iter()
            .any(|t| matches!(t.tool_name.as_str(), "send_message" | "send_voice"))
            || !output.response.is_empty();
        self.rounds.push(output);
        if did_send {
            self.schedule.refresh();
        }
        if matches!(
            self.engine.app.cfg.agent.context.session,
            SessionConfig::SingleRound
        ) {
            self.rounds.clear();
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

/// 派发一轮 agent task：spawn tokio task，通过 oneshot 返回 Round 结果。
///
/// 内部职责：
/// - 打印 prompt 日志
/// - spawn HeartbeatTask（RAII，drop 时自动取消）
/// - 调用 engine.run() 执行
/// - 标记消息已读
pub(super) fn spawn_round_task(
    engine: crate::agent::runtime::AgentEngine,
    ctx: RoundContext,
    payload: RoundTaskPayload,
) -> (JoinHandle<()>, oneshot::Receiver<proxy::RoundSignal>) {
    let bot = ctx.bot.clone();
    let chat_id = ctx.chat_id;
    let events_reasons: Vec<&str> = ctx.events.iter().map(|e| e.reason.label()).collect();
    let prompt = if tracing::enabled!(tracing::Level::INFO) {
        Some(payload.prompt.clone())
    } else {
        None
    };

    let shown_memory_ids = payload.shown_memory_ids.clone();
    let shown_topic_ids = payload.shown_topic_ids.clone();

    let (tx, rx) = oneshot::channel();

    let handle = tokio::spawn(async move {
        let started_at = tokio::time::Instant::now();
        tracing::info!(%chat_id, reasons = ?events_reasons, "Agent woke up");
        if let Some(p) = &prompt {
            tracing::info!(%chat_id, "Round prompt:\n{p}");
        }

        let _hb = proxy::HeartbeatTask::spawn(bot, chat_id);
        let result = engine.run(&ctx, payload.prompt).await;
        let elapsed = started_at.elapsed();

        match result {
            Ok(output) => {
                let tool_calls = output.tool_calls.len();
                let did_send = output.tool_calls.iter().any(|t| {
                    matches!(t.tool_name.as_str(), "send_message" | "send_voice")
                });
                let response_len = output.response.len();
                let response_truncated = if response_len > 200 {
                    format!("{}…({response_len})", &output.response[..200])
                } else {
                    output.response.clone()
                };
                tracing::info!(
                    %chat_id,
                    elapsed_secs = %elapsed.as_secs_f64(),
                    tool_calls,
                    did_send,
                    response_len,
                    response = %response_truncated,
                    "Agent done",
                );

                let msg_ids: Vec<crate::domain::vo::MessageId> = payload
                    .message_ids
                    .iter()
                    .map(|id| crate::domain::vo::MessageId(*id))
                    .collect();
                if !msg_ids.is_empty()
                    && let Err(e) = ctx.app.db.srv.message.mark_unread_seen(&msg_ids).await
                {
                    tracing::error!(%chat_id, "Failed to mark messages seen: {e}");
                }
                let _ = tx.send(Some(Round {
                    segment: payload.segment,
                    tool_calls: output.tool_calls,
                    response: output.response,
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
