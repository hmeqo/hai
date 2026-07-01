use std::sync::Arc;

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    agent::{context::account_element, runtime::tool_ctx::ToolContext},
    agentcore::{
        render::render_json,
        tool::{AgentTool, MapToolErr, ToolError, tool_data, tool_err},
    },
    domain::service::DbServices,
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetAccountInfoArgs {
    /// 用户 ID
    pub account_id: i64,
}

/// 获取用户信息
#[hai_macros::tool]
pub struct GetAccountInfo {
    pub services: DbServices,
}

impl GetAccountInfo {
    async fn exec(&self, args: GetAccountInfoArgs) -> Result<Value, ToolError> {
        let account = self
            .services
            .platform
            .get_account_by_id(crate::domain::vo::AccountId(args.account_id))
            .await
            .into_tool_err()?
            .ok_or_else(|| tool_err("账号不存在"))?;

        tool_data(serde_json::json!({ "account": render_json(account_element(&account)) }))
    }
}

pub fn tools(ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    vec![Arc::new(GetAccountInfo {
        services: ctx.db.clone(),
    })]
}
