use std::sync::Arc;

use genai::{
    Client,
    chat::{
        ChatMessage, ChatOptions, ChatRequest, ContentPart, MessageContent, Tool, ToolCall,
        ToolResponse,
    },
};

use super::{event::Inbox, types::Messages};
use crate::{
    agent::{
        context::build_situation_section,
        event::EventGroupSlice,
        node::main::build_react_config,
        runtime::{context::RunContext, engine::AgentEngine},
    },
    agentcore::{
        render::{Format, render_pretty},
        tool::{AgentTool, ToolError},
    },
    domain::vo::{AgentEventPayload, ChatId, ModelRetryReason, ToolCallResult, Turn, TurnOutput},
};

const DIRECT_OUTPUT_ERROR: &str =
    "Error: direct output is not allowed. Use send_message / send_voice to reply, or done to end.";

// ── Config ────────────────────────────────────────────────────────────────────

pub(crate) struct ReactLoopConfig {
    pub system_prompt: String,
    pub options: ChatOptions,
}

impl ReactLoopConfig {
    pub fn build_chat_options(cfg: &crate::config::schema::AgentConfig) -> ChatOptions {
        let mut opts = ChatOptions::default().with_temperature(cfg.temperature as f64);
        if let Some(maxt) = cfg.max_tokens {
            opts = opts.with_max_tokens(maxt);
        }
        if cfg.reasoning
            && let Some(effort) = cfg.reasoning_effort()
        {
            opts = opts.with_reasoning_effort(effort);
        }
        opts
    }
}

// ── Types ─────────────────────────────────────────────────────────────────────

pub(crate) struct ReactLoopOutput {
    pub turns: Vec<Turn>,
    pub messages: Messages,
}

/// 单次 run 所需的全部数据。
pub(crate) struct ReactRun {
    pub client: Client,
    pub model: String,
    pub messages: Messages,
    pub config: ReactLoopConfig,
    pub inbox: Inbox,
    pub preempt: bool,
    pub event_bus: super::AgentEventBus,
    pub chat_id: ChatId,
    pub run_number: usize,
}

impl ReactRun {
    pub(crate) fn new(
        engine: &AgentEngine,
        ctx: &RunContext,
        messages: Messages,
        inbox: Inbox,
        run_number: usize,
    ) -> Self {
        Self {
            client: engine.client.clone(),
            model: engine.model.clone(),
            messages,
            config: build_react_config(engine, ctx.chat_type),
            inbox,
            preempt: engine.app.cfg.agent.context.preempt,
            event_bus: engine.app.event_bus.clone(),
            chat_id: ctx.chat_id,
            run_number,
        }
    }
}

// ── React Loop ────────────────────────────────────────────────────────────────

#[allow(unused_assignments)]
pub(crate) async fn run_react_loop(
    mut run: ReactRun,
    tools: Vec<Arc<dyn AgentTool>>,
) -> Result<ReactLoopOutput, ToolError> {
    let mut turns: Vec<Turn> = Vec::new();
    let mut turn_index = 0;
    let genai_tools = prepare_genai_tools(&tools);

    loop {
        // ── 1. LLM call ──
        let res = llm_call_with_retry(&run, &genai_tools, &run.messages).await?;

        let response_text = res.texts().join("\n");
        let reasoning = res.reasoning_content.clone();
        let turn_prompt_tokens = res.usage.prompt_tokens.unwrap_or(0) as u32;
        let turn_completion_tokens = res.usage.completion_tokens.unwrap_or(0) as u32;
        let tool_calls: Vec<ToolCall> = res.into_tool_calls();
        let has_done = tool_calls.iter().any(|c| c.fn_name == "done");
        let active_calls = tool_calls;

        // ── 构建 assistant message ──
        run.messages.push(build_assistant_message(
            &response_text,
            &active_calls,
            reasoning.clone(),
        ));

        // ── 工具执行 ──
        let mut turn_tc: Vec<ToolCallResult> = Vec::new();
        for call in &active_calls {
            run.event_bus.emit(
                run.chat_id,
                AgentEventPayload::ToolCall {
                    run: run.run_number,
                    tool: call.fn_name.clone(),
                    args: call.fn_arguments.to_string(),
                },
            );
            execute_single_tool(call, &tools, &mut turn_tc, &mut run.messages).await;
            let result = turn_tc.last().unwrap();
            run.event_bus.emit(
                run.chat_id,
                AgentEventPayload::ToolCallResult {
                    run: run.run_number,
                    tool: call.fn_name.clone(),
                    summary: result.result.to_string(),
                    success: result.success,
                },
            );
        }

        // ──  Decide ──
        let mut stop = has_done;
        if !stop && active_calls.is_empty() {
            if response_text.trim().is_empty() {
                stop = true
            } else {
                run.event_bus.emit(
                    run.chat_id,
                    AgentEventPayload::ModelRetry {
                        run: run.run_number,
                        reason: ModelRetryReason::ResponseWithText,
                    },
                );
                run.messages.push(ChatMessage::user(DIRECT_OUTPUT_ERROR));
            }
        }

        // ── Commit ──
        turn_index += 1;
        turns.push(Turn {
            tool_calls: turn_tc,
            response: response_text,
            reasoning,
            prompt_tokens: turn_prompt_tokens,
            completion_tokens: turn_completion_tokens,
        });
        if !stop {
            let turn = turns.last().unwrap();
            run.event_bus.emit(
                run.chat_id,
                AgentEventPayload::TurnCompleted(TurnOutput {
                    run: run.run_number,
                    turn: turn_index,
                    reasoning: turn.reasoning.clone(),
                    response: turn.response.clone(),
                }),
            );
        }

        if stop {
            return Ok(ReactLoopOutput {
                turns,
                messages: run.messages,
            });
        }

        // ── Preempt ──
        if run.preempt
            && let Some(result) = inject_preempt(&mut run.messages, &run.inbox).await
        {
            run.event_bus.emit(
                run.chat_id,
                AgentEventPayload::Preempted {
                    run: run.run_number,
                    count: result.count,
                    reasons: result.reasons,
                    content: result.content,
                },
            );
        }
    }
}

// ── Preempt ───────────────────────────────────────────────────────────────────

pub(super) struct PreemptResult {
    pub count: usize,
    pub reasons: String,
    pub content: String,
}

async fn inject_preempt(messages: &mut Messages, inbox: &Inbox) -> Option<PreemptResult> {
    let events = inbox.drain();
    if events.is_empty() {
        return None;
    }
    let groups = events.coalesce();
    let content = render_pretty(build_situation_section(&groups), Format::Xml);
    messages.push(ChatMessage::user(content.clone()));
    Some(PreemptResult {
        count: events.len(),
        reasons: groups.reasons_summary(),
        content,
    })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn prepare_genai_tools(tools: &[Arc<dyn AgentTool>]) -> Vec<Tool> {
    tools
        .iter()
        .map(|t| {
            let mut tool = Tool::new(t.name()).with_description(t.description());
            if let Some(s) = t.schema() {
                tool = tool.with_schema(s);
            }
            tool
        })
        .collect()
}

fn build_assistant_message(
    response_text: &str,
    tool_calls: &[ToolCall],
    reasoning_content: Option<String>,
) -> ChatMessage {
    let mut parts = Vec::new();
    if !response_text.is_empty() {
        parts.push(ContentPart::from_text(response_text));
    }
    for call in tool_calls {
        parts.push(ContentPart::ToolCall(call.clone()));
    }
    let mut msg = ChatMessage::assistant(MessageContent::from_parts(parts));
    if let Some(r) = reasoning_content {
        msg = msg.with_reasoning_content(Some(r));
    }
    msg
}

/// 带重试的 LLM 调用。只对网络类错误重试，api/认证错误直接透传。
async fn llm_call_with_retry(
    run: &ReactRun,
    genai_tools: &[Tool],
    messages: &Messages,
) -> Result<genai::chat::ChatResponse, ToolError> {
    let max_retries = 2;
    let mut last_err = None;

    for attempt in 0..=max_retries {
        if attempt > 0 {
            run.event_bus.emit(
                run.chat_id,
                AgentEventPayload::ModelRetry {
                    run: run.run_number,
                    reason: ModelRetryReason::TimeoutRetry,
                },
            );
            tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
        }

        match run
            .client
            .exec_chat(
                &run.model,
                ChatRequest::new(messages.to_vec())
                    .with_system(&run.config.system_prompt)
                    .with_tools(genai_tools.to_vec()),
                Some(&run.config.options),
            )
            .await
        {
            Ok(res) => return Ok(res),
            Err(e) if is_retryable_ge(&e) => {
                let msg = format!("LLM request failed: {e}");
                last_err = Some(ToolError::Msg(msg));
            }
            Err(e) => return Err(ToolError::Msg(format!("LLM request failed: {e}"))),
        }
    }

    Err(last_err.unwrap_or_else(|| ToolError::Msg("LLM call failed after retries".into())))
}

fn is_retryable_ge(e: &genai::Error) -> bool {
    match e {
        genai::Error::WebModelCall { webc_error, .. }
        | genai::Error::WebAdapterCall { webc_error, .. } => match webc_error {
            genai::webc::Error::Reqwest(re) => re.is_timeout() || re.is_connect(),
            _ => false,
        },
        _ => false,
    }
}

async fn execute_single_tool(
    call: &ToolCall,
    tools: &[Arc<dyn AgentTool>],
    turn_tc: &mut Vec<ToolCallResult>,
    messages: &mut Messages,
) {
    let tool_name = &call.fn_name;
    let args = &call.fn_arguments;

    let result = match tools.iter().find(|t| t.name() == tool_name) {
        Some(tool) => tool.execute(args.clone()).await,
        None => Err(ToolError::Msg(format!("Unknown tool: {tool_name}"))),
    };

    match result {
        Ok(val) => {
            turn_tc.push(ToolCallResult::ok(
                tool_name.clone(),
                args.clone(),
                val.clone(),
            ));
            messages.push(ChatMessage::from(ToolResponse::from_tool_call(
                call,
                val.to_string(),
            )));
        }
        Err(e) => {
            turn_tc.push(ToolCallResult::err(
                tool_name.clone(),
                args.clone(),
                format!("{e}"),
            ));
            messages.push(ChatMessage::from(ToolResponse::from_tool_call(
                call,
                format!("Error: {e}"),
            )));
        }
    }
}
