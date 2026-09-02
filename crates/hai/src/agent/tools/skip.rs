use std::sync::Arc;

use serde_json::Value;

use crate::{
    agent::runtime::context::ToolContext,
    agentcore::tool::{AgentTool, ToolError, tool_ok},
};

/// 跳过本轮发言（不对外说话）。不发言 ≠ 不整理：有值得记住的信息/话题变化请先调用记忆/话题工具，再 skip。
#[hai_macros::tool(args = none)]
pub struct Skip;

impl Skip {
    async fn exec(&self) -> Result<Value, ToolError> {
        tool_ok()
    }
}

pub fn tools(_ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    vec![Arc::new(Skip)]
}
