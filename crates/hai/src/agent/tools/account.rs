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
        context::account_element,
        tools::{
            ToolContext,
            util::{MapToolErr, tool_data, tool_err},
        },
    },
    agentcore::render::render_json,
    domain::service::DbServices,
};

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
    pub services: DbServices,
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
            .into_tool_err()?
            .ok_or_else(|| tool_err("账号不存在"))?;

        tool_data(serde_json::json!({ "account": render_json(account_element(&account)) }))
    }
}

pub fn tools(ctx: &ToolContext) -> Vec<Arc<dyn ToolT>> {
    vec![Arc::new(GetAccountInfo {
        services: ctx.services(),
    })]
}
