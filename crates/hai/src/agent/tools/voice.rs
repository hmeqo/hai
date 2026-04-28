use std::sync::Arc;

use autoagents::{
    async_trait,
    core::tool::{ToolCallError, ToolInputT, ToolRuntime, ToolT},
};
use autoagents_derive::{ToolInput, tool};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    agent::{
        event::BotSignal,
        tools::{
            ToolContext,
            util::{MapToolErr, tool_ok},
        },
    },
    agentcore::multimodal::MultimodalService,
};

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct SendVoiceArgs {
    #[input(
        description = "传递给 TTS 模型的文本提示，建议详细描述说话风格场景和方式以生成更自然的语音"
    )]
    pub prompt: String,
    #[input(description = "归类到话题的 UUID")]
    pub topic_id: Option<Uuid>,
    #[input(description = "用于平台侧回复功能，指向某条具体消息的 ID")]
    pub platform_reply_to_id: Option<i64>,
}

#[tool(
    name = "send_voice",
    description = "发送语音消息。",
    input = SendVoiceArgs,
)]
pub struct SendVoice {
    pub chat_id: i64,
    pub signal_tx: tokio::sync::mpsc::UnboundedSender<BotSignal>,
    pub multimodal: MultimodalService,
}

#[async_trait]
impl ToolRuntime for SendVoice {
    async fn execute(&self, args: Value) -> std::result::Result<Value, ToolCallError> {
        let typed_args: SendVoiceArgs = serde_json::from_value(args)?;

        let audio_bytes = self
            .multimodal
            .speech(&typed_args.prompt)
            .await
            .into_tool_err()?;

        let _ = self.signal_tx.send(BotSignal::SendVoice {
            chat_id: self.chat_id,
            audio_bytes,
            prompt: typed_args.prompt,
            topic_id: typed_args.topic_id,
            platform_reply_to_id: typed_args.platform_reply_to_id,
        });
        tool_ok()
    }
}

pub fn get_voice_tools(ctx: &ToolContext) -> Vec<Arc<dyn ToolT>> {
    let tts_cfg = &ctx.ctx.cfg.multimodal.tts;
    if !tts_cfg.enabled() {
        return vec![];
    }

    vec![Arc::new(SendVoice {
        chat_id: ctx.chat_id,
        signal_tx: ctx.signal_tx.clone(),
        multimodal: ctx.ctx.provider.multimodal.clone(),
    })]
}
