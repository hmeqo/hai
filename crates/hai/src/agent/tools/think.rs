use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    agent::runtime::context::ToolContext,
    agentcore::tool::{AgentTool, ToolError, tool_ok},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ThinkArgs {
    #[allow(dead_code)]
    /// 当前思考内容
    pub thought: String,
}

/// 再想想，不产生任何外界效果。复杂推理或需要多想想的时候用。
#[hai_macros::tool]
pub struct Think;

impl Think {
    async fn exec(&self, _args: ThinkArgs) -> Result<Value, ToolError> {
        tool_ok()
    }
}

pub fn tools(_ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    vec![Arc::new(Think)]
}
