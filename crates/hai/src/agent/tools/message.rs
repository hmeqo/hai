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
        link::{BotConn, SendMessageReq},
        round::RoundContext,
        tools::util::{MapToolErr, tool_ok},
    },
    domain::service::DbServices,
};

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct SendMessageArgs {
    #[input(description = "消息内容")]
    pub content: String,
    #[input(description = "归类到话题的 UUID")]
    pub topic_id: Option<uuid::Uuid>,
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
    pub conn: BotConn,
    pub services: DbServices,
}

#[async_trait]
impl ToolRuntime for SendMessage {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: SendMessageArgs = serde_json::from_value(args)?;

        if let Some(ids) = &typed_args.replied_message_ids {
            self.services
                .topic
                .mark_as_replied(ids)
                .await
                .into_tool_err()?;
        }

        self.conn
            .send_message(SendMessageReq {
                chat_id: self.chat_id,
                content: typed_args.content,
                topic_id: typed_args.topic_id,
                platform_reply_to_id: typed_args.platform_reply_to_id,
            })
            .await
            .into_tool_err()?;

        tool_ok()
    }
}

pub fn get_message_tools(ctx: &RoundContext) -> Vec<Arc<dyn ToolT>> {
    vec![Arc::new(SendMessage {
        chat_id: ctx.chat_id,
        conn: ctx.conn.clone(),
        services: ctx.services(),
    })]
}
