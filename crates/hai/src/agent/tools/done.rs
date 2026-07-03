use std::sync::Arc;

use serde_json::Value;

use crate::{
    agent::runtime::context::ToolContext,
    agentcore::tool::{AgentTool, ToolError, tool_ok},
};

/// 无后续行动, 快速结束本轮。
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
