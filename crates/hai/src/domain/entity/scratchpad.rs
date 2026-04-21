use jiff::Timestamp;
use sqlx::FromRow;

/// Agent 的短期工作记忆，与 chat 一对一
///
/// 在每次 agent 运行结束时，通过结构化输出自动持久化。
/// 作用：防止消息窗口截断时 agent 遗忘近期的思考与重要中间状态。
/// 不取代长期记忆（memory 表），两者互补：
///   - scratchpad：短期、易变、随 agent 运行自动更新
///   - memory：长期、稳定、需 agent 主动调用工具写入
#[derive(Debug, Clone, FromRow)]
pub struct Scratchpad {
    pub chat_id: i64,
    pub content: String,
    pub token_count: i32,
    pub updated_at: jiff_sqlx::Timestamp,
}

impl Scratchpad {
    pub fn updated_at(&self) -> Timestamp {
        self.updated_at.to_jiff()
    }

    pub fn is_empty(&self) -> bool {
        self.content.trim().is_empty()
    }
}
