use std::sync::Arc;

use genai::{
    Client,
    chat::{
        ChatMessage, ChatOptions, ChatRequest, ContentPart, MessageContent, Tool, ToolCall,
        ToolResponse,
    },
};
use tokio::time::{Duration, Instant};

use super::{
    event::{Inbox, WakeEvents},
    types::Messages,
};
use crate::{
    agent::{
        event::WakeReason,
        node::main::build_react_config,
        runtime::{context::TurnContext, engine::AgentEngine},
    },
    agentcore::tool::{AgentTool, ToolError},
    domain::vo::{
        AgentEventPayload, ChatId, ModelRetryReason, Step, StepNumber, StepOutput, ToolCallResult,
        TurnNumber,
    },
};

/// 单个 react loop 的最大轮次（防模型无限调用工具/重试导致的死循环；超限强制结束，
/// 收尾 模式下无摘要则由上层判失败）。
const MAX_STEPS: usize = 20;

/// steering 防抖窗口：新 turn 启动后窗口内 Observe 类事件不打断（合并），
/// 与调度器 debounce（1500ms）同量级——turn 期间的新事件是注意力延续，窗口避免活锁。
const STEERING_WINDOW: Duration = Duration::from_millis(1500);

const DIRECT_OUTPUT_ERROR: &str =
    "Error: direct output is not allowed. Use send_message / send_voice to reply, or skip to end.";

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

/// react loop 执行模式：主循环（发言契约：必须经 send_message 发出）vs
/// 收尾（受限工具集、无 send_message，文本本身就是要收集的压缩摘要）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LoopMode {
    Main,
    WrapUp,
}

pub(crate) struct ReactLoopOutput {
    pub steps: Vec<Step>,
    pub messages: Messages,
    /// `Some(events)` = steering 提前正常结束（turn 期间新事件打断，事件回传续跑）。
    pub steered: Option<WakeEvents>,
}

/// 单次 turn 所需的全部数据。
pub(crate) struct ReactTurn {
    pub client: Client,
    pub model: String,
    pub messages: Messages,
    pub config: ReactLoopConfig,
    pub inbox: Inbox,
    /// steering 开关：turn 期间新事件是否打断续跑
    pub steering: bool,
    /// 防抖窗口截止：新 turn 启动 + STEERING_WINDOW，窗口内 Observe 不打断
    pub steering_until: Instant,
    pub mode: LoopMode,
    pub event_bus: super::AgentEventBus,
    pub chat_id: ChatId,
    pub turn_number: TurnNumber,
}

impl ReactTurn {
    pub(crate) fn new(
        engine: &AgentEngine,
        ctx: &TurnContext,
        messages: Messages,
        inbox: Inbox,
        turn_number: TurnNumber,
    ) -> Self {
        Self {
            client: engine.client.clone(),
            model: engine.model.clone(),
            messages,
            config: build_react_config(engine, ctx.chat_type),
            inbox,
            steering: engine.app.cfg.agent.context.steering,
            steering_until: Instant::now() + STEERING_WINDOW,
            mode: LoopMode::Main,
            event_bus: engine.app.event_bus.clone(),
            chat_id: ctx.chat_id,
            turn_number,
        }
    }
}

// ── React Loop ────────────────────────────────────────────────────────────────

#[allow(unused_assignments)]
pub(crate) async fn run_react_loop(
    mut turn: ReactTurn,
    tools: Vec<Arc<dyn AgentTool>>,
) -> Result<ReactLoopOutput, ToolError> {
    let mut steps: Vec<Step> = Vec::new();
    let mut turn_index = 0;
    let genai_tools = prepare_genai_tools(&tools);

    loop {
        // ── 1. LLM call ──
        let res = llm_call_with_retry(&turn, &genai_tools, &turn.messages).await?;

        let response_text = res.texts().join("\n");
        let reasoning = res.reasoning_content.clone();
        let turn_prompt_tokens = res.usage.prompt_tokens.unwrap_or(0) as u32;
        let turn_completion_tokens = res.usage.completion_tokens.unwrap_or(0) as u32;
        let tool_calls: Vec<ToolCall> = res.into_tool_calls();
        let has_skip = tool_calls.iter().any(|c| c.fn_name == "skip");
        let has_send_call = tool_calls
            .iter()
            .any(|c| matches!(c.fn_name.as_str(), "send_message" | "send_voice"));
        let non_skip_calls = tool_calls.iter().filter(|c| c.fn_name != "skip").count();
        let active_calls = tool_calls;

        // ── 构建 assistant message ──
        turn.messages.push(build_assistant_message(
            &response_text,
            &active_calls,
            reasoning.clone(),
        ));

        // ── Step 输出完成：模型响应先于工具调用（TUI 顺序 STEP → TOOL）──
        turn.event_bus.emit(
            turn.chat_id,
            AgentEventPayload::StepCompleted {
                turn: turn.turn_number,
                step: StepNumber::from(turn_index + 1),
                output: StepOutput {
                    turn: turn.turn_number,
                    step: StepNumber::from(turn_index + 1),
                    reasoning: reasoning.clone(),
                    response: response_text.clone(),
                },
            },
        );

        // ── 工具执行 ──
        let mut turn_tc: Vec<ToolCallResult> = Vec::new();
        for call in &active_calls {
            turn.event_bus.emit(
                turn.chat_id,
                AgentEventPayload::ToolCall {
                    turn: turn.turn_number,
                    step: StepNumber::from(turn_index + 1),
                    tool: call.fn_name.clone(),
                    args: call.fn_arguments.to_string(),
                },
            );
            execute_single_tool(call, &tools, &mut turn_tc, &mut turn.messages).await;
            let result = turn_tc.last().unwrap();
            turn.event_bus.emit(
                turn.chat_id,
                AgentEventPayload::ToolCallResult {
                    turn: turn.turn_number,
                    step: StepNumber::from(turn_index + 1),
                    tool: call.fn_name.clone(),
                    summary: result.result.to_string(),
                    success: result.success,
                },
            );
        }

        // ──  Decide ──
        // 主循环：发言必须经 send_message/send_voice；有文本但没发（含"文本 + skip"）→ 报错重试，不吞回复
        // 收尾：多轮整理——有工具调用则继续（整理记忆/话题），**无工具调用即停**（纯文本 = 最终摘要；
        //       空文本 + 无工具 = 无输出，由上层判失败）。不能"有文本即停"——模型首轮的
        //       整理声明（"我来整理…"）会被截断当摘要，丢失完整整理与最终摘要。
        let empty_text = response_text.trim().is_empty();

        let (stop, needs_retry) = match turn.mode {
            LoopMode::Main => {
                let stop = empty_text && (has_skip || active_calls.is_empty());
                let retry = !stop && !has_send_call && !empty_text && non_skip_calls == 0;
                (stop || turn_index + 1 >= MAX_STEPS, retry)
            }
            LoopMode::WrapUp => {
                // 无工具调用即停：文本非空 → 该文本即最终摘要；空 → 无摘要（上层判失败）
                let stop = active_calls.is_empty();
                (stop || turn_index + 1 >= MAX_STEPS, false)
            }
        };
        if needs_retry {
            turn.messages.push(ChatMessage::user(DIRECT_OUTPUT_ERROR));
        }

        // ── Commit ──
        turn_index += 1;
        steps.push(Step {
            tool_calls: turn_tc,
            response: response_text,
            reasoning,
            prompt_tokens: turn_prompt_tokens,
            completion_tokens: turn_completion_tokens,
        });

        // retry 提示在工具日志之后：先看到本轮模型输出与工具结果，再看到重试原因
        if needs_retry {
            turn.event_bus.emit(
                turn.chat_id,
                AgentEventPayload::ModelRetry {
                    turn: turn.turn_number,
                    reason: ModelRetryReason::ResponseWithText,
                },
            );
        }

        if stop {
            return Ok(ReactLoopOutput {
                steps,
                messages: turn.messages,
                steered: None,
            });
        }

        // ── Steering 检测（turn 期间新事件 = 注意力延续；收尾模式不响应）──
        // 提前正常结束当前 turn（已处理内容生效，上层推进状态后立即增量续跑新 turn），
        // turn 输入区间保持完整（不做中途 situation 注入）。
        if turn.steering && turn.inbox.len() > 0 {
            let events = turn.inbox.drain();
            let in_window = Instant::now() < turn.steering_until;
            let has_immediate = events
                .iter()
                .any(|e| !matches!(e.reason, WakeReason::Observe));
            if in_window && !has_immediate {
                // 防抖窗口内仅 Observe：不打断（事件放回，下轮再检）
                for e in events.iter() {
                    turn.inbox.push(e.clone());
                }
            } else {
                return Ok(ReactLoopOutput {
                    steps,
                    messages: turn.messages,
                    steered: Some(events),
                });
            }
        }
    }
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
    turn: &ReactTurn,
    genai_tools: &[Tool],
    messages: &Messages,
) -> Result<genai::chat::ChatResponse, ToolError> {
    let max_retries = 2;
    let mut last_err = None;

    for attempt in 0..=max_retries {
        if attempt > 0 {
            turn.event_bus.emit(
                turn.chat_id,
                AgentEventPayload::ModelRetry {
                    turn: turn.turn_number,
                    reason: ModelRetryReason::TimeoutRetry,
                },
            );
            tokio::time::sleep(std::time::Duration::from_millis(500 * attempt as u64)).await;
        }

        match turn
            .client
            .exec_chat(
                &turn.model,
                ChatRequest::new(messages.to_vec())
                    .with_system(&turn.config.system_prompt)
                    .with_tools(genai_tools.to_vec()),
                Some(&turn.config.options),
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
