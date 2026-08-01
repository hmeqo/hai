use std::{collections::HashSet, sync::Arc};

use tokio::sync::oneshot;
use uuid::Uuid;

use super::{
    context::RunContext,
    engine::AgentEngine,
    event::Inbox,
    react::{ReactRun, run_react_loop},
    session::{HeartbeatTask, RunInput},
    types::{BusySignal, Messages, RunOutput},
};
use crate::{
    agent::{context, link::BuiltContext, tools::get_main_agent_tools},
    domain::{
        model::Message,
        vo::{AgentEventPayload, MessageId, TurnOutput},
    },
    error::Result,
};

/// 执行引擎。只做 LLM 交互，不关心 session 生命周期。
pub(super) struct AgentRuntime {
    engine: AgentEngine,
    pub(super) handler: Arc<dyn crate::agent::link::PlatformHandler>,
    pub(super) shell: Arc<tokio::sync::Mutex<super::shell::ShellRuntime>>,
}

impl AgentRuntime {
    pub fn new(
        engine: &AgentEngine,
        handler: Arc<dyn crate::agent::link::PlatformHandler>,
        shell: Arc<tokio::sync::Mutex<super::shell::ShellRuntime>>,
    ) -> Self {
        Self {
            engine: engine.clone(),
            handler,
            shell,
        }
    }

    pub async fn build_prompt(
        &self,
        ctx: &RunContext,
        messages: &[Message],
        shown_memory_ids: &HashSet<Uuid>,
        shown_topic_ids: &HashSet<Uuid>,
        is_first: bool,
    ) -> Result<BuiltContext> {
        context::build_prompt(ctx, messages, shown_memory_ids, shown_topic_ids, is_first).await
    }

    /// 返回 (JoinHandle, result_rx)。rx 收到 `BusySignal::Run` 或 `BusySignal::Failed`。
    pub fn spawn_run(
        &self,
        ctx: RunContext,
        payload: RunInput,
        inbox: Inbox,
        run_number: usize,
    ) -> (
        tokio::task::JoinHandle<()>,
        tokio::sync::oneshot::Receiver<BusySignal>,
    ) {
        let handler = ctx.handler.clone();
        let chat_id = ctx.chat_id;
        let mut message_ids = payload.message_ids.clone();
        let since_id = payload.since_id;
        let event_bus = self.engine.app.event_bus.clone();

        let mc = &ctx.app.cfg.multimodal;
        let tts_enabled = mc.tts.enabled();
        let mut enabled_parsers = Vec::new();
        if mc.input.audio.enabled() {
            enabled_parsers.push(crate::domain::vo::AttachmentParser::Audio.name());
        }
        if mc.input.video.enabled() {
            enabled_parsers.push(crate::domain::vo::AttachmentParser::Video.name());
        }
        if mc.input.image.enabled() {
            enabled_parsers.push(crate::domain::vo::AttachmentParser::Image.name());
        }
        let sandbox_image = if ctx.app.cfg.sandbox.enabled {
            Some(ctx.app.cfg.sandbox.image.clone())
        } else {
            None
        };

        let run = ReactRun::new(&self.engine, &ctx, payload.messages, inbox, run_number);

        let (tx, rx) = oneshot::channel();

        let engine = self.engine.clone();
        let handle = tokio::spawn(async move {
            let started_at = tokio::time::Instant::now();

            let _hb = HeartbeatTask::spawn(handler, chat_id);

            let mut tools = get_main_agent_tools(
                &ctx.tool_ctx(),
                tts_enabled,
                &enabled_parsers,
                sandbox_image.as_deref(),
            );
            tools.extend(engine.mcp_manager.list_all_tools().await);

            let result = run_react_loop(run, tools).await;

            let elapsed = started_at.elapsed();

            let signal = match result {
                Ok(output) => {
                    let tool_calls: usize = output.turns.iter().map(|t| t.tool_calls.len()).sum();
                    let has_spoken =
                        output.turns.iter().flat_map(|t| &t.tool_calls).any(|tc| {
                            matches!(tc.tool_name.as_str(), "send_message" | "send_voice")
                        });

                    let prompt_tokens: u32 = output.turns.iter().map(|t| t.prompt_tokens).sum();
                    let completion_tokens: u32 =
                        output.turns.iter().map(|t| t.completion_tokens).sum();

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
                            prompt_tokens,
                            completion_tokens,
                            has_spoken,
                        },
                    );

                    if let Ok(extra) = ctx
                        .db
                        .message
                        .get_messages_window(chat_id, Some(since_id), 100)
                        .await
                    {
                        for m in &extra {
                            message_ids.push(m.id_());
                        }
                    }
                    let _ = mark_seen(&message_ids, &ctx.db).await;

                    BusySignal::Run(RunOutput {
                        messages: output.messages,
                        turns: output.turns,
                    })
                }
                Err(e) => {
                    tracing::warn!(
                        %chat_id,
                        elapsed_secs = %elapsed.as_secs_f64(),
                        error = %e,
                        "Agent run failed"
                    );
                    event_bus.emit(
                        chat_id,
                        AgentEventPayload::RunFailed {
                            run: run_number,
                            elapsed_ms: elapsed.as_millis() as u64,
                            error: e.to_string(),
                        },
                    );
                    BusySignal::Failed
                }
            };

            let _ = tx.send(signal);
        });

        (handle, rx)
    }

    /// 返回 (JoinHandle, result_rx)。rx 收到 `BusySignal::Compact` 或 `BusySignal::Failed`。
    pub fn spawn_compact(
        &self,
        messages: Messages,
    ) -> (
        tokio::task::JoinHandle<()>,
        tokio::sync::oneshot::Receiver<BusySignal>,
    ) {
        use genai::chat::{ChatMessage, ChatRequest};

        let client = self.engine.client.clone();
        let model = self.engine.model.clone();
        let (tx, rx) = oneshot::channel();

        let handle = tokio::spawn(async move {
            let mut msgs = messages.to_vec();
            msgs.push(ChatMessage::user(
                "请总结本轮的关键信息，用于下一轮恢复上下文。",
            ));
            let result = client
                .exec_chat(&model, ChatRequest::new(msgs), None)
                .await
                .map(|r| r.texts().join("\n"));

            let signal = match result {
                Ok(text) => BusySignal::Compact(text),
                Err(e) => {
                    tracing::warn!(error = %e, "Compact generation failed");
                    BusySignal::Failed
                }
            };

            let _ = tx.send(signal);
        });

        (handle, rx)
    }
}

async fn mark_seen(msg_ids: &[MessageId], db: &crate::domain::service::DbServices) -> Result<()> {
    if msg_ids.is_empty() {
        return Ok(());
    }
    db.message.mark_unread_seen(msg_ids).await?;
    Ok(())
}
