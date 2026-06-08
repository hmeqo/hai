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
    #[output(
        description = "内部笔记，不会被发送给任何人。如果已用 send_message 等工具交互则无需填写。"
    )]
    pub notes: Option<String>,
    #[serde(skip)]
    pub tool_calls: Vec<ToolCallResult>,
}

impl From<ReActAgentOutput> for MainAgentOutput {
    fn from(output: ReActAgentOutput) -> Self {
        let notes = output
            .try_parse::<MainAgentOutput>()
            .ok()
            .and_then(|m| m.notes)
            .or(Some(output.response));
        Self {
            notes,
            tool_calls: output.tool_calls,
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
        tracing::info!(
            tool = %result.tool_name,
            result = %result.result,
            "tool ok"
        );
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
