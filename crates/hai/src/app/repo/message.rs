use anyhow::Result;
use sqlx::PgPool;
use uuid::Uuid;

use crate::app::domain::entity::{Message, MessageStatus};

pub struct MessageRepo;

/// 创建消息所需的参数
pub struct CreateMessage<'a> {
    pub chat_id: i64,
    pub account_id: Option<i64>,
    pub role: &'a str,
    pub content: serde_json::Value,
    pub topic_id: Option<Uuid>,
    pub interaction_status: Option<&'a str>,
    pub reply_to_id: Option<i64>,
    pub external_id: Option<&'a str>,
    pub meta: Option<serde_json::Value>,
    pub token_count: Option<i32>,
    pub sent_at: Option<jiff_sqlx::Timestamp>,
}

impl MessageRepo {
    /// 插入一条新消息，返回创建的记录
    pub async fn create(pool: &PgPool, msg: CreateMessage<'_>) -> Result<Message> {
        // 1. 如果有 external_id，先尝试查询，避免直接 INSERT 导致 SERIAL 空洞
        if let Some(ext_id) = msg.external_id {
            let existing = sqlx::query_as!(
                Message,
                r#"
                SELECT
                    id, chat_id, account_id, role, content, topic_id as "topic_id: Uuid",
                    interaction_status as "interaction_status!",
                    reply_to_id, external_id, meta,
                    token_count, sent_at as "sent_at: jiff_sqlx::Timestamp",
                    created_at as "created_at!: jiff_sqlx::Timestamp",
                    updated_at as "updated_at!: jiff_sqlx::Timestamp"
                FROM message
                WHERE chat_id = $1 AND external_id = $2
                "#,
                msg.chat_id,
                ext_id,
            )
            .fetch_optional(pool)
            .await?;

            if let Some(m) = existing {
                // 2. 如果存在，更新 content 和 meta
                let updated = sqlx::query_as!(
                    Message,
                    r#"
                    UPDATE message
                    SET content = $2,
                        meta = COALESCE($3, meta)
                    WHERE id = $1
                    RETURNING
                        id, chat_id, account_id, role, content, topic_id as "topic_id: Uuid",
                        interaction_status as "interaction_status!",
                        reply_to_id, external_id, meta,
                        token_count, sent_at as "sent_at: jiff_sqlx::Timestamp",
                        created_at as "created_at!: jiff_sqlx::Timestamp",
                        updated_at as "updated_at!: jiff_sqlx::Timestamp"
                    "#,
                    m.id,
                    msg.content,
                    msg.meta,
                )
                .fetch_one(pool)
                .await?;
                return Ok(updated);
            }
        }

        // 3. 插入新消息（带上 ON CONFLICT 以防并发竞争）
        let row = sqlx::query_as!(
            Message,
            r#"
            INSERT INTO message (
                chat_id, account_id, role, content, topic_id,
                interaction_status,
                reply_to_id, external_id, meta, token_count,
                sent_at
            )
            VALUES ($1, $2, $3, $4, $5, COALESCE($6, 'pending'), $7, $8, $9, $10, $11)
            ON CONFLICT (chat_id, external_id) WHERE external_id IS NOT NULL DO UPDATE
                SET content = EXCLUDED.content,
                    meta = COALESCE(EXCLUDED.meta, message.meta)
            RETURNING
                id, chat_id, account_id, role, content, topic_id as "topic_id: Uuid",
                interaction_status as "interaction_status!",
                reply_to_id, external_id, meta,
                token_count, sent_at as "sent_at: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            "#,
            msg.chat_id,
            msg.account_id,
            msg.role,
            msg.content,
            msg.topic_id,
            msg.interaction_status,
            msg.reply_to_id,
            msg.external_id,
            msg.meta,
            msg.token_count,
            msg.sent_at as _,
        )
        .fetch_one(pool)
        .await?;

        // 4. 如果关联了话题，更新话题的最后活跃时间
        if let Some(topic_id) = row.topic_id {
            let active_at = row.active_at_sqlx();
            sqlx::query!(
                "UPDATE topic SET last_active_at = GREATEST(last_active_at, $1) WHERE id = $2",
                active_at as _,
                topic_id,
            )
            .execute(pool)
            .await?;
        }

        Ok(row)
    }

    /// 内部通用查询：按时间倒序取最新 N 条，再正序返回
    ///
    /// - `status_filter`: `None` 不过滤；`Some("pending")` 仅 pending；
    ///   其他值均视为"排除 pending"（已处理）
    async fn list_messages_ordered(
        pool: &PgPool,
        chat_id: i64,
        limit: i64,
        status_filter: Option<&str>,
    ) -> Result<Vec<Message>> {
        // sqlx 宏不支持动态 WHERE，分三种情况展开
        let rows = match status_filter {
            None => {
                sqlx::query_as!(
                    Message,
                    r#"
                    SELECT id, chat_id, account_id, role, content,
                        topic_id as "topic_id: Uuid",
                        interaction_status as "interaction_status!",
                        reply_to_id, external_id, meta,
                        token_count, sent_at as "sent_at: jiff_sqlx::Timestamp",
                        created_at as "created_at!: jiff_sqlx::Timestamp",
                        updated_at as "updated_at!: jiff_sqlx::Timestamp"
                    FROM (
                        SELECT * FROM message
                        WHERE chat_id = $1
                        ORDER BY COALESCE(sent_at, created_at) DESC, id DESC
                        LIMIT $2
                    ) AS sub
                    ORDER BY COALESCE(sent_at, created_at) ASC, id ASC
                    "#,
                    chat_id,
                    limit,
                )
                .fetch_all(pool)
                .await?
            }
            Some("pending") => {
                sqlx::query_as!(
                    Message,
                    r#"
                    SELECT id, chat_id, account_id, role, content,
                        topic_id as "topic_id: Uuid",
                        interaction_status as "interaction_status!",
                        reply_to_id, external_id, meta,
                        token_count, sent_at as "sent_at: jiff_sqlx::Timestamp",
                        created_at as "created_at!: jiff_sqlx::Timestamp",
                        updated_at as "updated_at!: jiff_sqlx::Timestamp"
                    FROM (
                        SELECT * FROM message
                        WHERE chat_id = $1 AND interaction_status = 'pending'
                        ORDER BY COALESCE(sent_at, created_at) DESC, id DESC
                        LIMIT $2
                    ) AS sub
                    ORDER BY COALESCE(sent_at, created_at) ASC, id ASC
                    "#,
                    chat_id,
                    limit,
                )
                .fetch_all(pool)
                .await?
            }
            _ => {
                sqlx::query_as!(
                    Message,
                    r#"
                    SELECT id, chat_id, account_id, role, content,
                        topic_id as "topic_id: Uuid",
                        interaction_status as "interaction_status!",
                        reply_to_id, external_id, meta,
                        token_count, sent_at as "sent_at: jiff_sqlx::Timestamp",
                        created_at as "created_at!: jiff_sqlx::Timestamp",
                        updated_at as "updated_at!: jiff_sqlx::Timestamp"
                    FROM (
                        SELECT * FROM message
                        WHERE chat_id = $1 AND interaction_status != 'pending'
                        ORDER BY COALESCE(sent_at, created_at) DESC, id DESC
                        LIMIT $2
                    ) AS sub
                    ORDER BY COALESCE(sent_at, created_at) ASC, id ASC
                    "#,
                    chat_id,
                    limit,
                )
                .fetch_all(pool)
                .await?
            }
        };
        Ok(rows)
    }

    /// 获取已处理历史消息（排除 pending），按时间正序返回
    /// 供 agent 工具主动拉取历史上下文
    pub async fn list_history_messages(
        pool: &PgPool,
        chat_id: i64,
        limit: i64,
    ) -> Result<Vec<Message>> {
        Self::list_messages_ordered(pool, chat_id, limit, Some("!pending")).await
    }

    /// 获取用于 agent 上下文的消息，同时返回 pending 消息的总数
    ///
    /// 返回 `(messages, total_pending)`：
    /// - messages: 已处理历史 + 全量 pending，按时间正序排列
    /// - total_pending: 该 chat 全部 pending 条数（用于提示 agent 还有多少未读）
    ///
    /// 策略：先取全部 pending（不受 limit 截断），再用剩余配额补充已处理消息
    pub async fn list_messages_for_context(
        pool: &PgPool,
        chat_id: i64,
        limit: i64,
    ) -> Result<(Vec<Message>, i64)> {
        let all_pending =
            Self::list_messages_ordered(pool, chat_id, i64::MAX, Some("pending")).await?;

        let total_pending = all_pending.len() as i64;
        let history_limit = (limit - total_pending).max(0);

        let mut history =
            Self::list_messages_ordered(pool, chat_id, history_limit, Some("!pending")).await?;

        history.extend(all_pending);
        Ok((history, total_pending))
    }

    /// 批量更新消息的 topic_id，并同步更新话题的起始时间
    pub async fn assign_topic(pool: &PgPool, message_ids: &[i64], topic_id: Uuid) -> Result<u64> {
        let mut tx = pool.begin().await?;

        // 1. 更新消息关联的话题，并标记为已阅
        let status: &'static str = MessageStatus::Seen.into();
        let result = sqlx::query!(
            "UPDATE message SET topic_id = $1, interaction_status = $2 WHERE id = ANY($3)",
            topic_id,
            status,
            message_ids,
        )
        .execute(&mut *tx)
        .await?;

        // 2. 同步更新话题的起始时间与最后活跃时间
        sqlx::query!(
            r#"
            UPDATE topic
            SET started_at = (SELECT MIN(COALESCE(sent_at, created_at)) FROM message WHERE topic_id = $1),
                last_active_at = (SELECT MAX(COALESCE(sent_at, created_at)) FROM message WHERE topic_id = $1)
            WHERE id = $1
            "#,
            topic_id,
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(result.rows_affected())
    }

    /// 批量标记消息状态
    async fn mark_status(pool: &PgPool, message_ids: &[i64], status: &str) -> Result<u64> {
        let result = sqlx::query!(
            "UPDATE message SET interaction_status = $1 WHERE id = ANY($2)",
            status,
            message_ids,
        )
        .execute(pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// 标记消息为已阅
    pub async fn mark_seen(pool: &PgPool, message_ids: &[i64]) -> Result<u64> {
        Self::mark_status(pool, message_ids, MessageStatus::Seen.into()).await
    }

    /// 标记消息为已回复
    pub async fn mark_replied(pool: &PgPool, message_ids: &[i64]) -> Result<u64> {
        Self::mark_status(pool, message_ids, MessageStatus::Replied.into()).await
    }

    /// 通过平台原始消息 ID 查找消息
    pub async fn find_by_external_id(
        pool: &PgPool,
        chat_id: i64,
        external_id: &str,
    ) -> Result<Option<Message>> {
        let row = sqlx::query_as!(
            Message,
            r#"
            SELECT
                id, chat_id, account_id, role, content,
                topic_id as "topic_id: Uuid",
                interaction_status as "interaction_status!",
                reply_to_id, external_id, meta,
                token_count, sent_at as "sent_at: jiff_sqlx::Timestamp",
                created_at as "created_at!: jiff_sqlx::Timestamp",
                updated_at as "updated_at!: jiff_sqlx::Timestamp"
            FROM message
            WHERE chat_id = $1 AND external_id = $2
            "#,
            chat_id,
            external_id,
        )
        .fetch_optional(pool)
        .await?;
        Ok(row)
    }
}
