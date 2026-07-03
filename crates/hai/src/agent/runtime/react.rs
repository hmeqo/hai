use std::sync::Arc;

use genai::{
    Client,
    chat::{
        ChatMessage, ChatOptions, ChatRequest, ContentPart, MessageContent, ReasoningEffort, Tool,
        ToolCall, ToolResponse,
    },
};

use super::types::{Inbox, Messages, ToolCallResult, Turn};
use super::AgentEvent;
use crate::{
    agent::context::build_situation_section,
    agentcore::{
        render::{Format, render_pretty},
        tool::{AgentTool, ToolError},
    },
    domain::vo::ChatId,
};

const DIRECT_OUTPUT_ERROR: &str =
    "错误：禁止直接输出文字。所有发言必须通过 send_message 或 send_voice 发送。不想说话请仅调用 done。";

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
            && let Some(effort) = ReasoningEffort::from_keyword(&cfg.reasoning_effort)
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
    pub prompt_tokens: u32,
}

/// 单次 processing 运行所需的全部数据。
pub(crate) struct ReactRun {
    pub client: Client,
    pub model: String,
    pub messages: Messages,
    pub config: ReactLoopConfig,
    pub inbox: Inbox,
    pub preempt: bool,
    pub event_bus: super::AgentEventBus,
    pub chat_id: ChatId,
    pub outer_turn: usize,
}

// ── React Loop ────────────────────────────────────────────────────────────────

#[allow(unused_assignments)]
pub(crate) async fn run_react_loop(
    run: ReactRun,
    tools: Vec<Arc<dyn AgentTool>>,
) -> Result<ReactLoopOutput, ToolError> {
    let mut messages = run.messages;
    let mut turns: Vec<Turn> = Vec::new();
    let mut prompt_tokens = 0u32;
    let genai_tools = prepare_genai_tools(&tools);

    loop {
        // ── 1. LLM call ──
        let res = run
            .client
            .exec_chat(
                &run.model,
                ChatRequest::new(messages.to_vec())
                    .with_system(&run.config.system_prompt)
                    .with_tools(genai_tools.clone()),
                Some(&run.config.options),
            )
            .await
            .map_err(|e| ToolError::Msg(format!("LLM request failed: {e}")))?;

        let response_text = res.texts().join("\n");
        let reasoning = res.reasoning_content.clone();
        prompt_tokens = res.usage.prompt_tokens.unwrap_or(0) as u32;
        let tool_calls: Vec<ToolCall> = res.into_tool_calls();

        let has_done = tool_calls.iter().any(|c| c.fn_name == "done");
        let active_calls: Vec<ToolCall> = tool_calls
            .into_iter()
            .filter(|c| c.fn_name != "done")
            .collect();

        // ── 2. 构建 assistant message ──
        messages.push(build_assistant_message(
            &response_text,
            &active_calls,
            reasoning.clone(),
        ));

        // ── 3. 工具执行 ──
        let mut turn_tc: Vec<ToolCallResult> = Vec::new();
        for call in &active_calls {
            run.event_bus.emit(AgentEvent::ToolCall {
                chat_id: run.chat_id,
                turn: run.outer_turn,
                tool: call.fn_name.clone(),
                args: call.fn_arguments.to_string(),
            });
            execute_single_tool(call, &tools, &mut turn_tc, &mut messages).await;
            let result = turn_tc.last().unwrap();
            run.event_bus.emit(AgentEvent::ToolCallResult {
                chat_id: run.chat_id,
                turn: run.outer_turn,
                tool: call.fn_name.clone(),
                summary: result.result.to_string(),
                success: result.success,
            });
        }

        // ── 4. 错误注入 ──
        if active_calls.is_empty() && !response_text.trim().is_empty() {
            messages.push(ChatMessage::user(DIRECT_OUTPUT_ERROR));
        }

        // ── 5. Decide（在 move 前判断） ──
        let stop = active_calls.is_empty() && response_text.trim().is_empty() || has_done;

        // ── 6. Commit ──
        turns.push(Turn {
            tool_calls: turn_tc,
            response: response_text,
            reasoning,
        });

        if stop {
            return Ok(ReactLoopOutput {
                turns,
                messages,
                prompt_tokens,
            });
        }

        // ── 7. Preempt ──
        if run.preempt {
            apply_preempt(&mut messages, &run.inbox).await;
        }
    }
}

// ── Preempt ───────────────────────────────────────────────────────────────────

async fn apply_preempt(messages: &mut Messages, inbox: &Inbox) -> bool {
    let events = inbox.drain();
    if events.is_empty() {
        return false;
    }
    let xml = render_pretty(build_situation_section(&events), Format::Xml);
    messages.push(ChatMessage::user(xml));
    true
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
            tracing::error!(tool = %tool_name, args = %args, error = %e, "tool error");
            turn_tc.push(ToolCallResult::err(tool_name.clone(), args.clone()));
            messages.push(ChatMessage::from(ToolResponse::from_tool_call(
                call,
                format!("Error: {e}"),
            )));
        }
    }
}
