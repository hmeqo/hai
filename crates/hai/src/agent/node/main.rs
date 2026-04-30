use std::sync::Arc;

use autoagents::{
    core::{
        agent::{AgentDeriveT, AgentHooks, Context},
        tool::{ToolCallResult, ToolT},
    },
    llm::ToolCall,
};
use autoagents_toolkit::mcp::McpToolWrapper;

#[derive(Debug, Clone)]
pub struct MainAgent {
    pub tools: Vec<Arc<dyn ToolT>>,
    pub system_prompt: String,
}

impl AgentDeriveT for MainAgent {
    type Output = String;

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

    fn output_schema(&self) -> Option<serde_json::Value> {
        None
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
        tracing::info!(
            tool = %result.tool_name,
            result = %result.result,
            "tool ok"
        );
    }

    async fn on_tool_error(&self, tool_call: &ToolCall, err: serde_json::Value, _ctx: &Context) {
        tracing::error!(
            tool = %tool_call.function.name,
            args = %tool_call.function.arguments,
            error = %err,
            "tool error"
        );
    }
}
