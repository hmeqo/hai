use std::sync::Arc;

use autoagents::{
    async_trait,
    core::tool::{ToolCallError, ToolInputT, ToolRuntime, ToolT},
};
use autoagents_derive::ToolInput;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    agent::{
        link::PlatformHandler,
        round::RoundContext,
        tools::util::{MapToolErr, tool_data, tool_err},
    },
    domain::vo::AttachmentParser,
};

#[derive(Serialize, Deserialize, ToolInput)]
pub struct AnalyzeAttachmentArgs {
    #[input(description = "附件 ID")]
    pub attachment_id: String,
    #[input(description = "可自定义识别要求，留空默认")]
    pub prompt: Option<String>,
}

#[derive(Debug)]
pub struct AnalyzeAttachment {
    handler: Arc<dyn PlatformHandler>,
    description: String,
}

impl AnalyzeAttachment {
    pub fn new(handler: Arc<dyn PlatformHandler>, extra_desc: &str) -> Self {
        Self {
            handler,
            description: format!("分析媒体内容。{extra_desc}"),
        }
    }
}

impl ToolT for AnalyzeAttachment {
    fn name(&self) -> &str {
        "analyze_attachment"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn args_schema(&self) -> Value {
        serde_json::from_str(AnalyzeAttachmentArgs::io_schema())
            .expect("Failed to parse parameters schema")
    }
}

#[async_trait]
impl ToolRuntime for AnalyzeAttachment {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let args: AnalyzeAttachmentArgs = serde_json::from_value(args)?;
        let uuid = Uuid::parse_str(&args.attachment_id)
            .map_err(|_| tool_err(format!("无效的 attachment_id: {}", args.attachment_id)))?;
        let result = self
            .handler
            .analyze_attachment(uuid, args.prompt.as_deref())
            .await
            .into_tool_err()?;
        tool_data(serde_json::json!({ "content": result }))
    }
}

pub fn multimodal_tools(ctx: &RoundContext) -> Vec<Arc<dyn ToolT>> {
    let mut enabled_parsers = Vec::new();
    if ctx.ctx.cfg.multimodal.input.audio.enabled() {
        enabled_parsers.push(AttachmentParser::Audio.name());
    }
    if ctx.ctx.cfg.multimodal.input.video.enabled() {
        enabled_parsers.push(AttachmentParser::Video.name());
    }
    if ctx.ctx.cfg.multimodal.input.image.enabled() {
        enabled_parsers.push(AttachmentParser::Image.name());
    }
    if enabled_parsers.is_empty() {
        return vec![];
    }
    let extra_desc = format!("仅支持解析类型：{:?}。", enabled_parsers.join(", "));
    vec![Arc::new(AnalyzeAttachment::new(
        ctx.conn.handler.clone(),
        &extra_desc,
    ))]
}
