use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    agent::{link::PlatformHandler, runtime::context::ToolContext},
    agentcore::tool::{AgentTool, MapToolErr, ToolError, tool_data, tool_err},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AnalyzeAttachmentArgs {
    /// 附件 ID
    pub attachment_id: String,
    /// 聚焦分析方向，留空默认全面分析
    pub prompt: Option<String>,
}

#[derive(Debug)]
pub struct AnalyzeAttachment {
    pub handler: Arc<dyn PlatformHandler>,
    pub description: String,
}

impl AnalyzeAttachment {
    pub fn new(handler: Arc<dyn PlatformHandler>, extra_desc: &str) -> Self {
        Self {
            handler,
            description: format!("分析媒体内容。{extra_desc}"),
        }
    }
}

#[async_trait]
impl AgentTool for AnalyzeAttachment {
    fn name(&self) -> &str {
        "analyze_attachment"
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn schema(&self) -> Option<Value> {
        Some(
            serde_json::to_value(schemars::schema_for!(AnalyzeAttachmentArgs))
                .expect("valid schema"),
        )
    }

    async fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let typed: AnalyzeAttachmentArgs = serde_json::from_value(args)?;
        let uuid = Uuid::parse_str(&typed.attachment_id)
            .map_err(|_| tool_err(format!("无效的 attachment_id: {}", typed.attachment_id)))?;
        let result = self
            .handler
            .analyze_attachment(uuid, typed.prompt.as_deref())
            .await
            .into_tool_err()?;
        tool_data(serde_json::json!({ "content": result }))
    }
}

pub fn tools(ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    if ctx.enabled_parsers.is_empty() {
        return vec![];
    }
    let extra_desc = format!("仅支持解析类型：{:?}。", ctx.enabled_parsers.join(", "));
    vec![Arc::new(AnalyzeAttachment::new(
        ctx.bot.handler.clone(),
        &extra_desc,
    ))]
}
