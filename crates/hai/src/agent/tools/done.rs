use std::sync::Arc;

use serde_json::Value;

use crate::{
    agent::runtime::context::ToolContext,
    agentcore::tool::{AgentTool, ToolError, tool_ok},
};

/// 没什么要说的了就结束。确认真的不需要再做什么了再用。
#[hai_macros::tool(args = none)]
pub struct Done;

impl Done {
    async fn exec(&self) -> Result<Value, ToolError> {
        tool_ok()
    }
}

pub fn tools(_ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    vec![Arc::new(Done)]
}
