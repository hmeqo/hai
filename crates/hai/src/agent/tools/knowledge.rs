//! 知识库检索工具：agent 主动检索全局知识库。

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    agent::{
        context::sections::knowledge::related_knowledge_section, runtime::context::ToolContext,
    },
    agentcore::{
        render::render_json,
        tool::{AgentTool, MapToolErr, ToolError, tool_data},
    },
    domain::service::DbServices,
};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SearchKnowledgeBaseArgs {
    pub query: String,
    pub limit: Option<i64>,
    /// 只搜指定 collection（空 = 全部库）
    pub collection: Option<String>,
}

/// 在全局知识库中语义检索。知识库是长期策展语料（非对话记忆），
/// 回答需要外部知识时使用。
#[hai_macros::tool]
pub struct SearchKnowledgeBase {
    pub services: DbServices,
}

impl SearchKnowledgeBase {
    async fn exec(&self, args: SearchKnowledgeBaseArgs) -> Result<Value, ToolError> {
        let limit = args.limit.unwrap_or(5).max(1);
        let collections: Vec<String> = args.collection.map(|c| vec![c]).unwrap_or_default();
        let hits = self
            .services
            .knowledge
            .search(&args.query, limit, &collections)
            .await
            .into_tool_err()?;

        let section = related_knowledge_section(&hits);
        tool_data(serde_json::json!({ "knowledge": render_json(section) }))
    }
}

pub fn tools(ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    vec![Arc::new(SearchKnowledgeBase {
        services: ctx.db.clone(),
    })]
}
