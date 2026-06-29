use std::sync::Arc;

use autoagents::{
    core::{
        agent::{
            AgentDeriveT, AgentHooks, AgentOutputT, Context, prebuilt::executor::ReActAgentOutput,
        },
        tool::{ToolCallResult, ToolT},
    },
    llm::ToolCall,
};
use autoagents_derive::AgentOutput;
use autoagents_toolkit::mcp::McpToolWrapper;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize, Deserialize, AgentOutput)]
pub struct MainAgentOutput {
    #[output()]
    #[serde(skip)]
    pub tool_calls: Vec<ToolCallResult>,
    pub response: String,
}

impl From<ReActAgentOutput> for MainAgentOutput {
    fn from(output: ReActAgentOutput) -> Self {
        Self {
            tool_calls: output.tool_calls,
            response: output.response,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MainAgent {
    pub tools: Vec<Arc<dyn ToolT>>,
    pub system_prompt: String,
}

impl AgentDeriveT for MainAgent {
    type Output = MainAgentOutput;

    fn name(&self) -> &str {
        "main_agent"
    }

    fn description(&self) -> &str {
        &self.system_prompt
    }

    fn tools(&self) -> Vec<Box<dyn ToolT>> {
        self.tools
            .iter()
            .map(|tool| Box::new(McpToolWrapper::new(Arc::clone(tool))) as Box<dyn ToolT>)
            .collect()
    }

    fn output_schema(&self) -> Option<Value> {
        Some(MainAgentOutput::output_schema().into())
    }
}

#[autoagents::async_trait]
impl AgentHooks for MainAgent {
    async fn on_tool_start(&self, tool_call: &ToolCall, _ctx: &Context) {
        tracing::info!(
            tool = %tool_call.function.name,
            args = %tool_call.function.arguments,
            "tool call"
        );
    }

    async fn on_tool_result(&self, _tool_call: &ToolCall, result: &ToolCallResult, _ctx: &Context) {
        let result_str = result.result.to_string();
        let result_len = result_str.len();
        let truncated = if result_len > 500 {
            format!("{}…(truncated, {result_len} chars)", &result_str[..500])
        } else {
            result_str
        };
        tracing::info!(
            tool = %result.tool_name,
            result = %truncated,
            result_len,
            "tool ok"
        );
        if result_len > 500 {
            tracing::debug!(
                tool = %result.tool_name,
                result = %result.result,
                "tool ok full"
            );
        }
    }

    async fn on_tool_error(&self, tool_call: &ToolCall, err: Value, _ctx: &Context) {
        tracing::error!(
            tool = %tool_call.function.name,
            args = %tool_call.function.arguments,
            error = %err,
            "tool error"
        );
    }
}
