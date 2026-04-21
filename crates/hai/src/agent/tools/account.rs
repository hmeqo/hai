use std::sync::Arc;

use anyhow::Result;
use autoagents::async_trait;
use autoagents::core::tool::{ToolCallError, ToolInputT, ToolRuntime, ToolT};
use autoagents_derive::{ToolInput, tool};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::components::account_element;
use crate::agent::render::render_json;
use crate::agent::tools::util::{ToolResult, toolcall_anyhow_err, toolcall_err};
use crate::domain::service::Services;

// --- Get Account Info Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct GetAccountInfoArgs {
    #[input(description = "用户 ID")]
    pub account_id: i64,
}

#[tool(
    name = "get_account_info",
    description = "获取用户信息",
    input = GetAccountInfoArgs,
)]
pub struct GetAccountInfo {
    pub services: Arc<Services>,
}

#[async_trait]
impl ToolRuntime for GetAccountInfo {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: GetAccountInfoArgs = serde_json::from_value(args)?;
        let account = self
            .services
            .platform
            .get_account_by_id(typed_args.account_id)
            .await
            .map_err(toolcall_anyhow_err)?
            .ok_or_else(|| toolcall_err("账号不存在"))?;

        Ok(ToolResult::success_with_data(
            "账号信息获取成功",
            serde_json::json!({ "account": render_json(account_element(&account)) }),
        )
        .to_value())
    }
}

pub fn tools(services: Arc<Services>) -> Vec<Arc<dyn ToolT>> {
    vec![Arc::new(GetAccountInfo {
        services: Arc::clone(&services),
    })]
}
