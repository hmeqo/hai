use autoagents::core::agent::AgentDeriveT;
use autoagents::core::tool::ToolT;
use autoagents_derive::AgentHooks;
use autoagents_toolkit::mcp::McpToolWrapper;
use std::sync::Arc;

#[derive(Debug, Clone, AgentHooks)]
pub struct MainAgent {
    pub tools: Vec<Arc<dyn ToolT>>,
    pub system_prompt: String,
}

impl AgentDeriveT for MainAgent {
    type Output = String;

    fn name(&self) -> &'static str {
        "main_agent"
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
