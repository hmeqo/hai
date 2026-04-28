use std::sync::Arc;

use autoagents::{
    async_trait,
    core::tool::{ToolCallError, ToolInputT, ToolRuntime, ToolT},
};
use autoagents_derive::{ToolInput, tool};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    agent::{
        event::BotSignal,
        tools::{
            ToolContext,
            util::{MapToolErr, tool_ok},
        },
    },
    domain::service::DbServices,
};

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct SendMessageArgs {
    #[input(description = "消息内容")]
    pub content: String,
    #[input(description = "归类到话题的 UUID")]
    pub topic_id: Option<Uuid>,
    #[input(description = "用于平台侧回复功能，指向某条具体消息的 ID")]
    pub platform_reply_to_id: Option<i64>,
    #[input(description = "同时标记回复(逻辑上)的消息 ID")]
    pub replied_message_ids: Option<Vec<i64>>,
}

#[tool(
    name = "send_message",
    description = "只在你想发言时考虑使用。",
    input = SendMessageArgs,
)]
pub struct SendMessage {
    pub chat_id: i64,
    pub signal_tx: mpsc::UnboundedSender<BotSignal>,
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for SendMessage {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: SendMessageArgs = serde_json::from_value(args)?;

        if let Some(ids) = &typed_args.replied_message_ids {
            let _ = self
                .services
                .topic
                .mark_as_replied(ids)
                .await
                .into_tool_err()?;
        }

        let _ = self.signal_tx.send(BotSignal::SendMessage {
            chat_id: self.chat_id,
            content: typed_args.content,
            topic_id: typed_args.topic_id,
            platform_reply_to_id: typed_args.platform_reply_to_id,
        });
        tool_ok()
    }
}

pub fn get_message_tools(ctx: &ToolContext) -> Vec<Arc<dyn ToolT>> {
    vec![Arc::new(SendMessage {
        chat_id: ctx.chat_id,
        signal_tx: ctx.signal_tx.clone(),
        services: ctx.services(),
    })]
}
