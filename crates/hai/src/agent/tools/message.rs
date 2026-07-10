use std::sync::Arc;

use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    agent::{
        link::{BotHandle, SendMessageReq},
        runtime::context::ToolContext,
        tools::util::deserialize_option_lenient_i64_vec,
    },
    agentcore::tool::{AgentTool, MapToolErr, ToolError, tool_data},
    domain::{service::DbServices, vo::ChatId},
};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SendMessageArgs {
    /// 消息内容
    pub content: String,
    /// 归类到话题的 UUID
    pub topic_id: Option<uuid::Uuid>,
    /// 用于平台侧回复功能，指向某条具体消息的 ID
    pub platform_reply_to_id: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_option_lenient_i64_vec")]
    /// 同时标记回复(逻辑上)的消息 ID
    pub replied_message_ids: Option<Vec<i64>>,
}

pub struct SendMessage {
    pub chat_id: ChatId,
    pub bot: BotHandle,
    pub services: DbServices,
}

#[async_trait]
impl AgentTool for SendMessage {
    fn name(&self) -> &str {
        "send_message"
    }

    fn description(&self) -> &str {
        if self.bot.rich_message {
            "发送消息，支持富文本"
        } else {
            "发送消息"
        }
    }

    fn schema(&self) -> Option<Value> {
        Some(serde_json::to_value(schemars::schema_for!(SendMessageArgs)).unwrap())
    }

    async fn execute(&self, args: Value) -> Result<Value, ToolError> {
        let args: SendMessageArgs = serde_json::from_value(args)?;

        if let Some(ids) = &args.replied_message_ids {
            let msg_ids: Vec<crate::domain::vo::MessageId> = ids
                .iter()
                .map(|id| crate::domain::vo::MessageId(*id))
                .collect();
            self.services
                .message
                .mark_replied(&msg_ids)
                .await
                .into_tool_err()?;
        }

        let meta = self
            .bot
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

pub fn tools(ctx: &ToolContext) -> Vec<Arc<dyn AgentTool>> {
    vec![Arc::new(SendMessage {
        chat_id: ctx.chat_id,
        bot: ctx.bot.clone(),
        services: ctx.db.clone(),
    })]
}
