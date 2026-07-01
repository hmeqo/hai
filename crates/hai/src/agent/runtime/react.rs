use std::sync::Arc;

use genai::{
    Client,
    chat::{
        ChatMessage, ChatOptions, ChatRequest, ContentPart, MessageContent, ReasoningEffort, Tool,
        ToolCall, ToolResponse,
    },
};

use super::types::{ToolCallResult, Turn};
use crate::agentcore::tool::{AgentTool, ToolError};

pub(crate) struct ReactLoopConfig {
    pub system_prompt: String,
    pub max_turns: usize,
    pub options: ChatOptions,
}

impl ReactLoopConfig {
    pub fn build_chat_options(cfg: &crate::config::schema::AgentConfig) -> ChatOptions {
        let mut opts = ChatOptions::default().with_temperature(cfg.temperature as f64);
        if let Some(maxt) = cfg.max_tokens {
            opts = opts.with_max_tokens(maxt);
        }
        if cfg.reasoning {
            if let Some(effort) = ReasoningEffort::from_keyword(&cfg.reasoning_effort) {
                opts = opts.with_reasoning_effort(effort);
            }
        }
        opts
    }
}

pub(crate) async fn run_react_loop(
    client: Client,
    model: &str,
    messages: Vec<ChatMessage>,
    tools: Vec<Arc<dyn AgentTool>>,
    config: &ReactLoopConfig,
) -> Result<ReactLoopOutput, ToolError> {
    let mut messages = messages;
    let mut all_results: Vec<ToolCallResult> = Vec::new();
    let mut turns: Vec<Turn> = Vec::new();
    let genai_tools = prepare_genai_tools(&tools);

    let mut last_text = String::new();
    let mut fr_retries = 0usize;

    for _turn in 0..config.max_turns {
        let req = ChatRequest::new(messages.clone())
            .with_system(&config.system_prompt)
            .with_tools(genai_tools.clone());

        let res = client
            .exec_chat(model, req, Some(&config.options))
            .await
            .map_err(|e| ToolError::Msg(format!("LLM request failed: {e}")))?;

        let response_text = res.texts().join("\n");
        let reasoning = res.reasoning_content.clone();
        let usage = res.usage.clone();
        let stop_reason = res
            .stop_reason
            .as_ref()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let tool_calls: Vec<ToolCall> = res.into_tool_calls();
        let tc_before = all_results.len();

        if tool_calls.is_empty() {
            if response_text.trim().is_empty() {
                turns.push(Turn {
                    tool_calls: all_results[tc_before..].to_vec(),
                    usage,
                    stop_reason,
                });
                return Ok(ReactLoopOutput {
                    turns,
                    messages,
                    final_response: String::new(),
                });
            }
            fr_retries += 1;
            if fr_retries > 3 {
                turns.push(Turn {
                    tool_calls: all_results[tc_before..].to_vec(),
                    usage,
                    stop_reason,
                });
                return Ok(ReactLoopOutput {
                    turns,
                    messages,
                    final_response: response_text.trim().to_owned(),
                });
            }
            tracing::info!(
                retry = fr_retries,
                response = %response_text.trim(),
                usage = ?usage,
                "Final Response 非空，已丢弃并重试",
            );
            // 失败的 Turn 仍记录（消耗了 token）
            turns.push(Turn {
                tool_calls: Vec::new(),
                usage,
                stop_reason,
            });
            messages.push(build_assistant_message(&response_text, &[], reasoning));
            messages.push(ChatMessage::user(
                "错误：Final Response 必须为空。刚才的输出已被丢弃。",
            ));
            continue;
        }

        last_text = response_text;
        messages.push(build_assistant_message(&last_text, &tool_calls, reasoning));

        for call in &tool_calls {
            if call.fn_name == "done" {
                tracing::info!("Agent called done, ending loop");
                turns.push(Turn {
                    tool_calls: Vec::new(),
                    usage,
                    stop_reason,
                });
                return Ok(ReactLoopOutput {
                    turns,
                    messages,
                    final_response: String::new(),
                });
            }
            execute_single_tool(call, &tools, &mut all_results, &mut messages).await;
        }

        turns.push(Turn {
            tool_calls: all_results[tc_before..].to_vec(),
            usage,
            stop_reason,
        });
    }

    Ok(ReactLoopOutput {
        turns,
        messages,
        final_response: last_text.trim().to_owned(),
    })
}

pub(crate) struct ReactLoopOutput {
    pub turns: Vec<Turn>,
    pub messages: Vec<ChatMessage>,
    pub final_response: String,
}

// ── 辅助函数 ─────────────────────────────────────────────────────────────────

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
    all_results: &mut Vec<ToolCallResult>,
    messages: &mut Vec<ChatMessage>,
) {
    let tool_name = &call.fn_name;
    let args = &call.fn_arguments;

    tracing::info!(tool = %tool_name, args = %args, "tool call");

    let result = match tools.iter().find(|t| t.name() == tool_name) {
        Some(tool) => tool.execute(args.clone()).await,
        None => Err(ToolError::Msg(format!("Unknown tool: {tool_name}"))),
    };

    match result {
        Ok(val) => {
            tracing::info!(tool = %tool_name, result = %val, "tool ok");
            all_results.push(ToolCallResult::ok(
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
            all_results.push(ToolCallResult::err(tool_name.clone(), args.clone()));
            messages.push(ChatMessage::from(ToolResponse::from_tool_call(
                call,
                format!("Error: {e}"),
            )));
        }
    }
}
