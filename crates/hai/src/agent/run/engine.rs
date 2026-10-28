use std::{sync::Arc, time::Duration};

use genai::chat::ChatMessage;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use super::{
    Messages, TurnOutput,
    agent_loop::{LoopTurn, TurnIdentity, run_loop},
    has_spoken,
    step_loop::StepLoopOutput,
    turn::{SelectedModel, ToolContext, TurnContext},
};
use crate::{
    agent::{
        chat::passes::BusySignal,
        events::Doorbell,
        process::AgentRuntime,
        prompt::role::{self, AgentRole},
        tools,
    },
    agentcore::tool::{AgentTool, ToolError},
    domain::vo::{
        AgentEvent, AgentEventPayload, SessionId, SessionRef, StepNumber, StepOutput,
        TurnEndReason, TurnNumber,
    },
};

pub(crate) struct TurnRun<'a> {
    pub messages: Messages,
    pub doorbell: Doorbell,
    pub steering_since: u64,
    pub turn_number: TurnNumber,
    pub session: SessionRef,
    pub model: &'a SelectedModel,
}

pub(crate) struct AgentEngine {
    pub(crate) runtime: AgentRuntime,
    /// The chat's own shutdown; the run selects on it.
    shutdown: CancellationToken,
}

/// The tools one chat mounts, built once. A digest pass is handed the reads-only subset before its
/// loop starts (`agent/chat/passes/digest.rs:ConversationAgent::start_digest`); every other role
/// narrows the set at call time
/// (`RoleSpec::rejects`).
pub(crate) fn mounted_tools(
    runtime: &AgentRuntime,
    ctx: &ToolContext,
) -> Arc<[Arc<dyn AgentTool>]> {
    let mut assembled = role::default::tools(runtime, ctx);
    assembled.extend(runtime.mcp_manager.tools().iter().cloned());
    tools::sanitize(assembled, &runtime.cfg().tools.disabled).into()
}

impl AgentEngine {
    pub fn new(runtime: &AgentRuntime, shutdown: CancellationToken) -> Self {
        Self {
            runtime: runtime.clone(),
            shutdown,
        }
    }

    pub(crate) fn shutdown(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    /// One busy task: mount the tools, run the loop, hand the outcome to `finish` and send its signal
    /// back. The run's own role says whether the chat waits on it, and that is what the typing
    /// indicator covers.
    fn spawn_busy(
        &self,
        tools: Arc<[Arc<dyn AgentTool>]>,
        turn: LoopTurn,
        ctx: TurnContext,
        finish: impl FnOnce(
            Result<StepLoopOutput, ToolError>,
            Duration,
            &[Arc<dyn AgentTool>],
        ) -> BusySignal
        + Send
        + 'static,
    ) -> tokio::sync::oneshot::Receiver<BusySignal> {
        let chat = ctx.tool.chat.clone();
        let typing = turn.spec.shows_typing();
        let shutdown = self.shutdown.clone();
        let (tx, rx) = oneshot::channel();

        tokio::spawn(async move {
            let started_at = tokio::time::Instant::now();
            let _typing = if typing {
                chat.start_typing().await
            } else {
                None
            };

            let signal = tokio::select! {
                biased;
                // A stopping process wins over a result that was ready at the same moment, so what a
                // run ends up doing never depends on poll order.
                _ = shutdown.cancelled() => BusySignal::Cancelled,
                result = run_loop(turn, &tools) => finish(result, started_at.elapsed(), &tools),
            };

            let _ = tx.send(signal);
        });

        rx
    }

    pub fn spawn_turn(
        &self,
        ctx: TurnContext,
        tools: Arc<[Arc<dyn AgentTool>]>,
        run: TurnRun<'_>,
    ) -> tokio::sync::oneshot::Receiver<BusySignal> {
        let TurnRun {
            messages,
            doorbell,
            steering_since,
            turn_number,
            session,
            model,
        } = run;
        let chat_id = ctx.tool.chat.chat_id();
        let event_bus = self.runtime.event_bus().clone();

        let spec = AgentRole::Default.spec(
            &self.runtime,
            model.chat_options(),
            ctx.chat_type,
            ctx.language,
            ctx.persona.as_deref(),
            ctx.content_policy,
        );
        let turn = LoopTurn::new(
            &self.runtime,
            model,
            spec,
            TurnIdentity {
                chat_id,
                turn_number,
                session,
            },
            messages,
            doorbell,
            steering_since,
        );

        self.spawn_busy(
            tools,
            turn,
            ctx,
            move |result, elapsed, mounted| match result {
                Ok(output) => {
                    let StepLoopOutput {
                        steps,
                        messages,
                        steered,
                        truncated: _,
                    } = output;
                    let tool_calls: usize = steps.iter().map(|t| t.tool_calls.len()).sum();
                    let has_spoken = has_spoken(&steps, mounted);

                    let context_tokens: u32 = steps.last().map(|t| t.prompt_tokens).unwrap_or(0);

                    let last = steps.last().map(|t| StepOutput {
                        turn: turn_number,
                        step: StepNumber::from(steps.len()),
                        reasoning: t.reasoning.clone(),
                        response: t.response.clone(),
                    });
                    let last = last.expect("invariant: loop commits >= 1 step");

                    let output = TurnOutput {
                        messages,
                        steps,
                        has_spoken,
                    };
                    let (reason, signal) = match steered {
                        true => (
                            TurnEndReason::Steered {
                                output: last,
                                tool_calls,
                                elapsed_ms: elapsed.as_millis() as u64,
                                context_tokens,
                                has_spoken,
                            },
                            BusySignal::Steered(output),
                        ),
                        false => (
                            TurnEndReason::Success {
                                output: last,
                                tool_calls,
                                elapsed_ms: elapsed.as_millis() as u64,
                                context_tokens,
                                has_spoken,
                            },
                            BusySignal::Turn(output),
                        ),
                    };
                    event_bus.emit(AgentEvent {
                        chat_id,
                        session: Some(session),
                        payload: AgentEventPayload::TurnEnded {
                            turn: turn_number,
                            reason,
                        },
                    });

                    signal
                }
                Err(e) => {
                    tracing::warn!(
                        %chat_id,
                        elapsed_secs = %elapsed.as_secs_f64(),
                        error = %e,
                        "Agent turn failed"
                    );
                    event_bus.emit(AgentEvent {
                        chat_id,
                        session: Some(session),
                        payload: AgentEventPayload::TurnEnded {
                            turn: turn_number,
                            reason: TurnEndReason::Failed {
                                error: e.to_string(),
                            },
                        },
                    });
                    BusySignal::TurnFailed
                }
            },
        )
    }

    pub fn spawn_wrap_up(
        &self,
        ctx: TurnContext,
        tools: Arc<[Arc<dyn AgentTool>]>,
        messages: Messages,
        doorbell: Doorbell,
        parent: SessionId,
        model: &SelectedModel,
    ) -> tokio::sync::oneshot::Receiver<BusySignal> {
        let mut wrap_messages = messages;
        wrap_messages.push(ChatMessage::user(
            self.runtime.prompts.wrap_up(ctx.language).to_owned(),
        ));
        let spec = AgentRole::WrapUp.spec(
            &self.runtime,
            model.chat_options(),
            ctx.chat_type,
            ctx.language,
            None,
            ctx.content_policy,
        );
        let chat_id = ctx.tool.chat.chat_id();
        let session = SessionRef::wrap_up(SessionId::new(), parent);
        let turn = LoopTurn::new(
            &self.runtime,
            model,
            spec,
            TurnIdentity {
                chat_id,
                turn_number: TurnNumber::from(1),
                session,
            },
            wrap_messages,
            doorbell,
            0,
        );

        let event_bus = self.runtime.event_bus().clone();
        event_bus.emit(AgentEvent {
            chat_id,
            session: Some(session),
            payload: AgentEventPayload::WrapUpStarted,
        });

        self.spawn_busy(
            tools,
            turn,
            ctx,
            move |result, _elapsed, _mounted| match result {
                Ok(output) => {
                    let context_tokens = output.steps.last().map(|t| t.prompt_tokens).unwrap_or(0);
                    let text = output
                        .steps
                        .iter()
                        .rev()
                        .filter_map(|t| role::wrap_up::extract_summary(&t.response))
                        .next()
                        .or_else(|| {
                            output
                                .steps
                                .iter()
                                .rev()
                                .find(|t| !t.response.trim().is_empty())
                                .map(|t| t.response.clone())
                        });
                    match text {
                        Some(summary) => BusySignal::WrapUp {
                            summary,
                            context_tokens,
                            session,
                        },
                        None => BusySignal::WrapUpFailed {
                            error:
                                "no summary (the model produced no substantive compaction summary)"
                                    .into(),
                            session,
                        },
                    }
                }
                Err(e) => BusySignal::WrapUpFailed {
                    error: e.to_string(),
                    session,
                },
            },
        )
    }
}
