use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    agent::{
        link::{MessageCapability, SendImageReq, SendMessageReq, SendVoiceReq},
        multimodal::MultimodalService,
        runtime::context::ToolContext,
    },
    agentcore::{
        apiclient::ImageUrl,
        tool::{AgentTool, MapToolErr, ToolError, tool_data, tool_ok},
    },
    domain::vo::ChatId,
};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SendMessageArgs {
    pub content: String,
    pub topic_id: Option<Uuid>,
    /// 用于平台侧回复功能，指向某条具体消息的 ID
    pub platform_reply_to_id: Option<i64>,
}

pub struct SendMessage {
    pub chat_id: ChatId,
    pub handler: Arc<dyn crate::agent::link::PlatformHandler>,
}

#[async_trait]
impl AgentTool for SendMessage {
    fn name(&self) -> &str {
        "send_message"
    }

    fn description(&self) -> &str {
        match self.handler.message_capability() {
            MessageCapability::Rich => "发送消息（支持富文本）",
            _ => "发送消息",
        }
    }

    fn schema(&self) -> Option<Value> {
        Some(serde_json::to_value(schemars::schema_for!(SendMessageArgs)).unwrap())
    }

    async fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let args: SendMessageArgs = serde_json::from_value(args)?;

        let meta = self
            .handler
            .send_message(SendMessageReq {
                chat_id: self.chat_id,
                content: args.content,
                topic_id: args.topic_id,
                platform_reply_to_id: args.platform_reply_to_id,
            })
            .await
            .into_tool_err()?;

        tool_data(json!({
            "message_id": meta.external_id
        }))
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SendVoiceArgs {
    /// 传递给 TTS 模型的文本提示，建议详细描述说话风格场景和方式以生成更自然的语音
    pub prompt: String,
    pub topic_id: Option<Uuid>,
    /// 用于平台侧回复功能，指向某条具体消息的 ID
    pub platform_reply_to_id: Option<i64>,
}

/// 发送语音消息。
#[hai_macros::tool]
pub struct SendVoice {
    pub chat_id: ChatId,
    pub handler: Arc<dyn crate::agent::link::PlatformHandler>,
    pub multimodal: MultimodalService,
}

impl SendVoice {
    async fn exec(&self, args: SendVoiceArgs) -> Result<Value, ToolError> {
        let audio_bytes = self.multimodal.speech(&args.prompt).await.into_tool_err()?;

        self.handler
            .send_voice(SendVoiceReq {
                chat_id: self.chat_id,
                audio_bytes,
                prompt: args.prompt,
                topic_id: args.topic_id,
                platform_reply_to_id: args.platform_reply_to_id,
            })
            .await
            .into_tool_err()?;

        tool_ok()
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GenerateImageArgs {
    /// 图像生成提示词，详细描述想生成的画面内容、风格、构图
    pub prompt: String,
    /// 随图片发送给用户的说明文字
    pub caption: Option<String>,
    /// 参考图 URL 列表
    pub images: Option<Vec<String>>,
    pub topic_id: Option<Uuid>,
    /// 用于平台侧回复功能，指向某条具体消息的 ID
    pub platform_reply_to_id: Option<i64>,
}

/// 生成一张图片并发送给用户。
/// 生成成功后图片会直接发到聊天；文本回复可用 send_message 补充说明。
#[hai_macros::tool]
pub struct GenerateImage {
    pub chat_id: ChatId,
    pub handler: Arc<dyn crate::agent::link::PlatformHandler>,
    pub multimodal: MultimodalService,
}

impl GenerateImage {
    async fn exec(&self, args: GenerateImageArgs) -> Result<Value, ToolError> {
        let images: Vec<_> = args
            .images
            .unwrap_or_default()
            .into_iter()
            .map(ImageUrl)
            .collect();
        let image_bytes = self
            .multimodal
            .generate_image(&args.prompt, &images)
            .await
            .into_tool_err()?;

        self.handler
            .send_image(SendImageReq {
                chat_id: self.chat_id,
                image_bytes,
                prompt: args.prompt,
                caption: args.caption,
                topic_id: args.topic_id,
                platform_reply_to_id: args.platform_reply_to_id,
            })
            .await
            .into_tool_err()?;

        tool_ok()
    }
}

pub fn tools(ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    let mut all: Vec<Arc<dyn AgentTool>> = Vec::new();
    all.push(Arc::new(SendMessage {
        chat_id: ctx.chat_id,
        handler: ctx.handler.clone(),
    }));
    if ctx.multimodal.tts_enabled() {
        all.push(Arc::new(SendVoice {
            chat_id: ctx.chat_id,
            handler: ctx.handler.clone(),
            multimodal: ctx.multimodal.clone(),
        }));
    }
    // 条件挂载：image-gen 配了模型才挂（image_gen 服务可用）
    if ctx.multimodal.image_gen_enabled() {
        all.push(Arc::new(GenerateImage {
            chat_id: ctx.chat_id,
            handler: ctx.handler.clone(),
            multimodal: ctx.multimodal.clone(),
        }));
    }
    all
}
