use std::sync::Arc;

use anyhow::Result;
use autoagents::async_trait;
use autoagents::core::tool::{ToolCallError, ToolInputT, ToolRuntime, ToolT};
use autoagents_derive::{ToolInput, tool};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::agent::event::BotSignal;
use crate::agent::tools::util::{ToolResult, toolcall_anyhow_err};
use crate::domain::service::{Services, TopicService};

// --- Send Message Tool ---

#[derive(Debug, Serialize, Deserialize, ToolInput)]
pub struct SendMessageArgs {
    #[input(description = "消息内容")]
    pub content: String,
    #[input(description = "所属话题 UUID")]
    pub topic_id: Option<Uuid>,
    #[input(description = "用于平台侧回复功能，指向某条具体消息的 ID")]
    pub platform_reply_to_id: Option<i64>,
    #[input(description = "同时标记回复的消息 ID")]
    pub replied_message_ids: Option<Vec<i64>>,
}

#[tool(
    name = "send_message",
    description = "想说话时才调用。可直接发或回复，可同时标记已阅/已回复，指定 topic_id 自动归类。不需要发言时不要调用此工具。",
    input = SendMessageArgs,
)]
pub struct SendMessage {
    pub chat_id: i64,
    pub signal_tx: mpsc::UnboundedSender<BotSignal>,
    pub topic_service: Arc<TopicService>,
}

#[async_trait]
impl ToolRuntime for SendMessage {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let typed_args: SendMessageArgs = serde_json::from_value(args)?;

        tracing::info!(
            chat_id = self.chat_id,
            content = %typed_args.content,
            "Agent sent message"
        );

        if let Some(ids) = &typed_args.replied_message_ids {
            let _ = self
                .topic_service
                .mark_as_replied(ids)
                .await
                .map_err(toolcall_anyhow_err)?;
        }

        let _ = self.signal_tx.send(BotSignal::SendMessage {
            chat_id: self.chat_id,
            content: typed_args.content,
            topic_id: typed_args.topic_id,
            platform_reply_to_id: typed_args.platform_reply_to_id,
        });
        Ok(ToolResult::success("消息已通过异步通道发送成功").to_value())
    }
}

pub fn get_message_tools(
    services: Arc<Services>,
    chat_id: i64,
    signal_tx: mpsc::UnboundedSender<BotSignal>,
) -> Vec<Arc<dyn ToolT>> {
    vec![Arc::new(SendMessage {
        chat_id,
        signal_tx,
        topic_service: Arc::clone(&services.topic),
    })]
}
