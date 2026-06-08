use std::sync::Arc;

use autoagents::{
    async_trait,
    core::tool::{ToolCallError, ToolInputT, ToolRuntime, ToolT},
};
use autoagents_derive::{ToolInput, tool};
use kameo::actor::ActorRef;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    agent::{
        link::SendMessageReq,
        runtime::{actor::ChatActor, ctx::RoundCtx},
        tools::util::{MapToolErr, deserialize_option_lenient_i64_vec, tool_data},
    },
    domain::{service::DbServices, vo::ChatId},
    error::{AppResultExt, ErrorKind},
};

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct SendMessageArgs {
    #[input(description = "消息内容")]
    pub content: String,
    #[input(description = "归类到话题的 UUID")]
    pub topic_id: Option<uuid::Uuid>,
    #[input(description = "用于平台侧回复功能，指向某条具体消息的 ID")]
    pub platform_reply_to_id: Option<i64>,
    #[serde(default, deserialize_with = "deserialize_option_lenient_i64_vec")]
    #[input(description = "同时标记回复(逻辑上)的消息 ID")]
    pub replied_message_ids: Option<Vec<i64>>,
}

#[tool(
    name = "send_message",
    description = "发消息。",
    input = SendMessageArgs,
)]
pub struct SendMessage {
    pub chat_id: ChatId,
    pub session: ActorRef<ChatActor>,
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

        let meta = self
            .session
            .ask(SendMessageReq {
                chat_id: self.chat_id,
                content: typed_args.content,
                topic_id: typed_args.topic_id,
                platform_reply_to_id: typed_args.platform_reply_to_id,
            })
            .await
            .err_kind_msg(ErrorKind::Internal, "Session mailbox error")
            .into_tool_err()?;

        tool_data(json!({
            "message_id": meta.external_id
        }))
    }
}

pub fn get_message_tools(ctx: &RoundCtx) -> Vec<Arc<dyn ToolT>> {
    vec![Arc::new(SendMessage {
        chat_id: ctx.chat_id,
        session: ctx.session.clone(),
        services: ctx.services(),
    })]
}
