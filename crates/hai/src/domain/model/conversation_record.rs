#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ConversationRecord {
    pub chat_id: i64,

    pub messages: serde_json::Value,

    /// 对话级状态元信息（since_id——消息游标，跨章节；未来对话级字段进此列，零迁移）。
    pub state: serde_json::Value,

    /// 章节级状态元信息（shown ids + ContextMeta 标量；重开章节整体归零重写，未来章节级字段进此列）。
    pub context_meta: serde_json::Value,
    #[sqlx(try_from = "jiff_sqlx::Timestamp")]
    pub updated_at: jiff::Timestamp,
}
