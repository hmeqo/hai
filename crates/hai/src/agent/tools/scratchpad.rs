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
        round::RoundContext,
        tools::util::{MapToolErr, tool_ok},
    },
    domain::service::DbServices,
};

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct UpdateScratchpadArgs {
    #[input(description = "新的内容")]
    pub content: String,
}

#[tool(
    name = "update_scratchpad",
    description = "更新草稿板。结束时调用一次就行，把值得延续到下一轮的信息写进去。",
    input = UpdateScratchpadArgs,
)]
pub struct UpdateScratchpad {
    pub chat_id: i64,
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for UpdateScratchpad {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let args: UpdateScratchpadArgs = serde_json::from_value(args)?;

        self.services
            .scratchpad
            .save(self.chat_id, &args.content)
            .await
            .into_tool_err()?;

        tool_ok()
    }
}

pub fn tools(ctx: &RoundContext) -> Vec<Arc<dyn ToolT>> {
    vec![Arc::new(UpdateScratchpad {
        chat_id: ctx.chat_id,
        services: ctx.services(),
    })]
}
