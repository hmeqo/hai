use anyhow::Result;
use sqlx::PgPool;
use tiktoken_rs::cl100k_base;

use crate::app::{
    domain::{
        entity::{Message, MessageStatus},
        model::AgentMessageMeta,
    },
    repo::message::{CreateMessage, MessageRepo},
};

/// 消息管理服务
pub struct MessageService {
    pool: PgPool,
}

impl MessageService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 估算 JSON 内容的 token 数量
    fn estimate_tokens(content: &serde_json::Value) -> Result<i32> {
        let s = content.to_string();
        let count = cl100k_base()?.encode_with_special_tokens(&s).len() as i32;
        Ok(count)
    }

    /// 保存用户发送的消息
    pub async fn save_user_message(
        &self,
        chat_id: i64,
        account_id: i64,
        content: serde_json::Value,
        external_id: &str,
        reply_to_id: Option<i64>,
        meta: Option<serde_json::Value>,
        sent_at: Option<jiff_sqlx::Timestamp>,
    ) -> Result<Message> {
        let token_count = Self::estimate_tokens(&content)?;

        MessageRepo::create(
            &self.pool,
            CreateMessage {
                chat_id,
                account_id: Some(account_id),
                role: "user",
                content,
                topic_id: None,
                interaction_status: Some(MessageStatus::Pending.into()),
                reply_to_id,
                external_id: Some(external_id),
                meta,
                token_count: Some(token_count),
                sent_at,
            },
        )
        .await
    }

    /// 保存 Agent 发送的消息
    pub async fn save_agent_message(
        &self,
        chat_id: i64,
        account_id: Option<i64>,
        content: serde_json::Value,
        model: &str,
        tokens: i32,
        reply_to_id: Option<i64>,
        external_id: Option<&str>,
        sent_at: Option<jiff_sqlx::Timestamp>,
    ) -> Result<Message> {
        // 若调用方未提供 token 数，则自行估算
        let token_count = if tokens > 0 {
            tokens
        } else {
            Self::estimate_tokens(&content)?
        };

        let meta_value = serde_json::to_value(AgentMessageMeta {
            model: model.to_string(),
        })?;

        MessageRepo::create(
            &self.pool,
            CreateMessage {
                chat_id,
                account_id,
                role: "assistant",
                content,
                topic_id: None,
                interaction_status: Some(MessageStatus::Seen.into()),
                reply_to_id,
                external_id,
                meta: Some(meta_value),
                token_count: Some(token_count),
                sent_at,
            },
        )
        .await
    }

    /// 获取 agent 上下文消息：先取全量 pending，再用剩余配额补到 limit 条已处理消息
    ///
    /// 返回 `(messages, total_pending)` —— `total_pending` 可能大于 messages 中实际
    /// pending 数，表示还有更多 pending 超出渲染窗口，agent 可通过工具获取。
    pub async fn get_messages(&self, chat_id: i64, limit: i64) -> Result<(Vec<Message>, i64)> {
        MessageRepo::list_messages_for_context(&self.pool, chat_id, limit).await
    }

    /// 获取历史已处理消息（排除 pending），供 agent 工具主动拉取历史上下文
    pub async fn get_history_messages(&self, chat_id: i64, limit: i64) -> Result<Vec<Message>> {
        MessageRepo::list_history_messages(&self.pool, chat_id, limit).await
    }

    /// 通过平台原始消息 ID 查找内部消息 ID
    pub async fn find_id_by_external_id(
        &self,
        chat_id: i64,
        external_id: &str,
    ) -> Result<Option<i64>> {
        let msg = MessageRepo::find_by_external_id(&self.pool, chat_id, external_id).await?;
        Ok(msg.map(|m| m.id))
    }
}
