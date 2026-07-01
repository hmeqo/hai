use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    agent::runtime::context::ToolContext,
    agentcore::tool::{AgentTool, MapToolErr, ToolError, tool_ok},
    domain::{service::DbServices, vo::ChatId},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateScratchpadArgs {
    /// 新的内容
    #[serde(default)]
    pub content: String,
}

/// 更新你的主观工作记忆（草稿板），用于跨轮次延续思考进度。每次处理消息时先回顾再更新，已完成的及时清理。
#[hai_macros::tool]
pub struct UpdateScratchpad {
    pub chat_id: ChatId,
    pub services: DbServices,
}

impl UpdateScratchpad {
    async fn exec(&self, args: UpdateScratchpadArgs) -> Result<Value, ToolError> {
        self.services
            .scratchpad
            .save(self.chat_id, &args.content)
            .await
            .into_tool_err()?;

        tool_ok()
    }
}

pub fn tools(ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    vec![Arc::new(UpdateScratchpad {
        chat_id: ctx.chat_id,
        services: ctx.db.clone(),
    })]
}
