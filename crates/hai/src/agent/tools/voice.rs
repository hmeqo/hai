use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    agent::{
        link::{BotHandle, SendVoiceReq},
        node::MultimodalService,
        runtime::tool_ctx::ToolContext,
    },
    agentcore::tool::{AgentTool, MapToolErr, ToolError, tool_ok},
    domain::vo::ChatId,
};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SendVoiceArgs {
    /// 传递给 TTS 模型的文本提示，建议详细描述说话风格场景和方式以生成更自然的语音
    pub prompt: String,
    /// 归类到话题的 UUID
    pub topic_id: Option<Uuid>,
    /// 用于平台侧回复功能，指向某条具体消息的 ID
    pub platform_reply_to_id: Option<i64>,
}

/// 发送语音消息。
#[hai_macros::tool]
pub struct SendVoice {
    pub chat_id: ChatId,
    pub bot: BotHandle,
    pub multimodal: MultimodalService,
}

impl SendVoice {
    async fn exec(&self, args: SendVoiceArgs) -> Result<Value, ToolError> {
        let audio_bytes = self.multimodal.speech(&args.prompt).await.into_tool_err()?;

        self.bot
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

pub fn tools(ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    if !ctx.tts_enabled {
        return vec![];
    }

    vec![Arc::new(SendVoice {
        chat_id: ctx.chat_id,
        bot: ctx.bot.clone(),
        multimodal: ctx.multimodal.clone(),
    })]
}
