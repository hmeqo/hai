use sqlx::PgPool;
use uuid::Uuid;

use crate::{
    agentcore::token::count_json_tokens,
    domain::{
        entity::{Message, MessageStatus},
        repo::message::{CreateMessage, MessageRepo},
        vo::{AgentMessageMeta, MessageMeta, TelegramContentPart},
    },
    error::Result,
};

/// 保存用户消息所需参数
pub struct NewUserMessage<'a> {
    pub chat_id: i64,
    pub account_id: i64,
    pub content: serde_json::Value,
    pub external_id: &'a str,
    pub reply_to_id: Option<i64>,
    pub meta: MessageMeta,
    pub sent_at: Option<jiff_sqlx::Timestamp>,
}

/// 保存 Agent 消息所需参数
pub struct NewAgentMessage<'a> {
    pub chat_id: i64,
    pub account_id: Option<i64>,
    pub content: serde_json::Value,
    pub model: &'a str,
    /// 调用方已知 token 数时传入（0 表示自动估算）
    pub tokens: i32,
    pub reply_to_id: Option<i64>,
    pub external_id: Option<&'a str>,
    pub sent_at: Option<jiff_sqlx::Timestamp>,
}

/// 消息管理服务
#[derive(Debug)]
pub struct MessageService {
    pool: PgPool,
}

impl MessageService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// 估算 JSON 内容的 token 数量
    fn estimate_tokens(content: &serde_json::Value) -> Result<i32> {
        Ok(count_json_tokens(content) as i32)
    }

    /// 保存用户发送的消息
    pub async fn save_user_message(&self, msg: NewUserMessage<'_>) -> Result<Message> {
        let token_count = Self::estimate_tokens(&msg.content)?;

        MessageRepo::create(
            &self.pool,
            CreateMessage {
                chat_id: msg.chat_id,
                account_id: Some(msg.account_id),
                role: "user",
                content: msg.content,
                topic_id: None,
                interaction_status: Some(MessageStatus::Pending.into()),
                reply_to_id: msg.reply_to_id,
                external_id: Some(msg.external_id),
                meta: serde_json::to_value(&msg.meta).unwrap_or(serde_json::Value::Null),
                token_count: Some(token_count),
                sent_at: msg.sent_at,
            },
        )
        .await
    }

    /// 保存 Agent 发送的消息
    pub async fn save_agent_message(&self, msg: NewAgentMessage<'_>) -> Result<Message> {
        // 若调用方未提供 token 数，则自行估算
        let token_count = if msg.tokens > 0 {
            msg.tokens
        } else {
            Self::estimate_tokens(&msg.content)?
        };

        let meta = AgentMessageMeta {
            model: msg.model.to_string(),
        };

        MessageRepo::create(
            &self.pool,
            CreateMessage {
                chat_id: msg.chat_id,
                account_id: msg.account_id,
                role: "assistant",
                content: msg.content,
                topic_id: None,
                interaction_status: Some(MessageStatus::Seen.into()),
                reply_to_id: msg.reply_to_id,
                external_id: msg.external_id,
                meta: serde_json::to_value(&meta).unwrap_or(serde_json::Value::Null),
                token_count: Some(token_count),
                sent_at: msg.sent_at,
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

    /// 更新消息的 meta 字段
    pub async fn update_message_meta(
        &self,
        message_id: i64,
        meta: Option<serde_json::Value>,
    ) -> Result<()> {
        MessageRepo::update_meta(&self.pool, message_id, meta).await
    }

    /// 通过 attachment_id 查找对应的消息及匹配的内容部分
    ///
    /// 扫描 content JSONB 数组，找到含有指定 attachment_id 的 part。
    /// 返回 `(message, matched_part)`，找不到返回 `None`。
    /// 通过 attachment_id 查找对应的消息及匹配的内容部分。
    pub async fn find_by_attachment_id(
        &self,
        attachment_id: Uuid,
    ) -> Result<Option<(Message, TelegramContentPart)>> {
        // attachment_id 是 UUID，每条消息最多对应一个，fetch_optional 即可
        let msg = sqlx::query_as::<_, Message>(
            r#"
            SELECT
                id, chat_id, account_id, role, content,
                topic_id,
                interaction_status,
                reply_to_id, external_id, meta,
                token_count, sent_at,
                created_at, updated_at
            FROM message
            WHERE content @> $1::jsonb
            "#,
        )
        .bind(serde_json::json!([{"attachment_id": attachment_id}]))
        .fetch_optional(&self.pool)
        .await?;

        let Some(msg) = msg else { return Ok(None) };

        let part = serde_json::from_value::<Vec<TelegramContentPart>>(msg.content.clone())?
            .into_iter()
            .find(|p| p.attachment_id() == Some(attachment_id));

        Ok(part.map(|p| (msg, p)))
    }
}
