use std::sync::Arc;

use autoagents::{core::agent::AgentDeriveT, prelude::*};
use autoagents_derive::AgentHooks;
use autoagents_toolkit::mcp::McpToolWrapper;

#[derive(Debug, Clone, AgentHooks)]
pub struct McpAgent {
    pub tools: Vec<Arc<dyn ToolT>>,
    pub system_prompt: String,
}

impl AgentDeriveT for McpAgent {
    type Output = String;

    fn name(&self) -> &'static str {
        "mcp_agent"
    }

    fn description(&self) -> &'static str {
        unsafe { std::mem::transmute::<&str, &'static str>(self.system_prompt.as_str()) }
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
