use std::sync::Arc;

use genai::{
    Client,
    chat::{ChatMessage, ChatRequest, ContentPart, MessageContent, Tool, ToolCall, ToolResponse},
};

use super::round::ToolCallResult;
use crate::agentcore::tool::{AgentTool, ToolError};

pub(crate) struct ReactLoop {
    client: Client,
    model: String,
    max_turns: usize,
}

impl ReactLoop {
    pub fn new(client: Client, model: impl Into<String>, max_turns: usize) -> Self {
        Self {
            client,
            model: model.into(),
            max_turns,
        }
    }

    pub async fn run(
        self,
        messages: Vec<ChatMessage>,
        tools: Vec<Arc<dyn AgentTool>>,
    ) -> Result<ReactLoopOutput, ToolError> {
        let mut messages = messages;
        let mut all_results: Vec<ToolCallResult> = Vec::new();
        let genai_tools = prepare_genai_tools(&tools);

        let mut last_text = String::new();
        let mut fr_retries = 0usize;

        for _turn in 0..self.max_turns {
            let req = ChatRequest::new(messages.clone()).with_tools(genai_tools.clone());

            let res = self
                .client
                .exec_chat(&self.model, req, None)
                .await
                .map_err(|e| ToolError::Msg(format!("LLM request failed: {e}")))?;

            let response_text = res.texts().join("\n");
            let tool_calls: Vec<ToolCall> = res.into_tool_calls();

            if tool_calls.is_empty() {
                if response_text.trim().is_empty() {
                    return Ok(ReactLoopOutput {
                        tool_calls: all_results,
                        messages,
                        final_response: String::new(),
                    });
                }
                fr_retries += 1;
                if fr_retries > 3 {
                    return Ok(ReactLoopOutput {
                        tool_calls: all_results,
                        messages,
                        final_response: response_text.trim().to_owned(),
                    });
                }
                tracing::info!(
                    retry = fr_retries,
                    response = %response_text.trim(),
                    "Final Response 非空，已丢弃并重试",
                );
                messages.push(build_assistant_message(&response_text, &[]));
                messages.push(ChatMessage::user(
                    "错误：Final Response 必须为空。刚才的输出已被丢弃。",
                ));
                continue;
            }

            last_text = response_text;
            messages.push(build_assistant_message(&last_text, &tool_calls));

            for call in &tool_calls {
                execute_single_tool(call, &tools, &mut all_results, &mut messages).await;
            }
        }

        Ok(ReactLoopOutput {
            tool_calls: all_results,
            messages,
            final_response: last_text.trim().to_owned(),
        })
    }
}

pub(crate) struct ReactLoopOutput {
    pub tool_calls: Vec<ToolCallResult>,
    pub messages: Vec<ChatMessage>,
    /// ReAct 循环结束时的最终输出文本。
    pub final_response: String,
}

// ── 辅助函数 ─────────────────────────────────────────────────────────────────

fn prepare_genai_tools(tools: &[Arc<dyn AgentTool>]) -> Vec<Tool> {
    tools
        .iter()
        .map(|t| {
            Tool::new(t.name())
                .with_description(t.description())
                .with_schema(t.schema())
        })
        .collect()
}

fn build_assistant_message(response_text: &str, tool_calls: &[ToolCall]) -> ChatMessage {
    let mut parts = Vec::new();
    if !response_text.is_empty() {
        parts.push(ContentPart::from_text(response_text));
    }
    for call in tool_calls {
        parts.push(ContentPart::ToolCall(call.clone()));
    }
    ChatMessage::assistant(MessageContent::from_parts(parts))
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
