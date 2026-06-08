use std::sync::Arc;

use autoagents::{
    async_trait,
    core::tool::{ToolCallError, ToolInputT, ToolRuntime, ToolT},
};
use autoagents_derive::{ToolInput, tool};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    agent::{
        runtime::ctx::RoundCtx,
        tools::util::{MapToolErr, tool_ok},
    },
    domain::{service::DbServices, vo::ChatId},
};

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct UpdateScratchpadArgs {
    #[input(description = "chat_id")]
    pub chat_id: i64,
    #[input(description = "新的内容")]
    pub content: String,
}

#[tool(
    name = "update_scratchpad",
    description = "更新你的主观工作记忆（草稿板），用于跨轮次延续思考进度。每次处理消息时先回顾再更新，已完成的及时清理。",
    input = UpdateScratchpadArgs,
)]
pub struct UpdateScratchpad {
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for UpdateScratchpad {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let args: UpdateScratchpadArgs = serde_json::from_value(args)?;

        self.services
            .scratchpad
            .save(ChatId::from(args.chat_id), &args.content)
            .await
            .into_tool_err()?;

        tool_ok()
    }
}

pub fn tools(ctx: &RoundCtx) -> Vec<Arc<dyn ToolT>> {
    vec![Arc::new(UpdateScratchpad {
        services: ctx.services(),
    })]
}
