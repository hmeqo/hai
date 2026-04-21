use anyhow::Result;
use sqlx::PgPool;

use crate::domain::{
    entity::{Message, MessageStatus},
    repo::message::{CreateMessage, MessageRepo},
    vo::AgentMessageMeta,
};
use crate::util::token::count_json_tokens;

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
        Ok(count_json_tokens(content)? as i32)
    }

    /// 保存用户发送的消息
    #[allow(clippy::too_many_arguments)]
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
    #[allow(clippy::too_many_arguments)]
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
    pub async fn get_messages_for_context(
        &self,
        chat_id: i64,
        limit: i64,
    ) -> Result<(Vec<Message>, i64)> {
        MessageRepo::get_messages_for_context(&self.pool, chat_id, limit).await
    }

    /// 通过内部消息 ID 获取消息
    pub async fn get_message_by_id(&self, id: i64) -> Result<Option<Message>> {
        MessageRepo::find_by_id(&self.pool, id).await
    }

    /// 批量按 ID 获取消息
    pub async fn get_messages_by_ids(&self, ids: &[i64]) -> Result<Vec<Message>> {
        MessageRepo::find_by_ids(&self.pool, ids).await
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

    /// 标记指定消息中未标记的为已阅（自动过滤已标记的）
    pub async fn mark_unread_seen(&self, message_ids: &[i64]) -> Result<u64> {
        MessageRepo::mark_unread_seen(&self.pool, message_ids).await
    }
}
