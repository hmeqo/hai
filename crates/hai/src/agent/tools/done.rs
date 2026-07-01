use std::sync::Arc;

use serde_json::Value;

use crate::{
    agent::runtime::context::ToolContext,
    agentcore::tool::{AgentTool, ToolError, tool_ok},
};

/// 评估完消息后决定本轮结束。调用此工具表示你说完了或不参与，不再生成额外输出。
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
