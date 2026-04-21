use std::sync::Arc;

use anyhow::Result;
use autoagents::async_trait;
use autoagents::core::tool::{ToolCallError, ToolInputT, ToolRuntime, ToolT};
use autoagents_derive::{ToolInput, tool};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::tools::util::{ToolResult, toolcall_anyhow_err};
use crate::domain::service::Services;

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct UpdateScratchpadArgs {
    #[input(description = "chat_id")]
    pub chat_id: i64,
    #[input(description = "新的草稿板内容，留空字符串表示清空")]
    pub content: String,
}

#[tool(
    name = "update_scratchpad",
    description = "更新草稿板",
    input = UpdateScratchpadArgs,
)]
pub struct UpdateScratchpad {
    pub services: Arc<Services>,
}

#[async_trait]
impl ToolRuntime for UpdateScratchpad {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let args: UpdateScratchpadArgs = serde_json::from_value(args)?;

        self.services
            .scratchpad
            .save(args.chat_id, &args.content)
            .await
            .map_err(toolcall_anyhow_err)?;

        Ok(ToolResult::success("草稿板已更新").to_value())
    }
}

pub fn tools(services: Arc<Services>) -> Vec<Arc<dyn ToolT>> {
    vec![Arc::new(UpdateScratchpad { services })]
}
