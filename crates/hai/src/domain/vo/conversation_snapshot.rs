use std::collections::HashSet;

use genai::chat::ChatMessage;
use uuid::Uuid;

use crate::domain::vo::{ContextMeta, MessageId};

/// `Conversation` 的纯数据快照（持久化形态）。
///
/// `context_messages`（上下文消息）持久化——恢复无缝续接，
/// token 阈值限制大小（重开清空）；`ContextMeta` 为章节级标量聚合。
#[derive(Clone)]
pub struct ConversationSnapshot {
    /// 给 LLM 的累积消息序列（含注入/工具链；重开后 = 收尾摘要或空）。
    pub context_messages: Vec<ChatMessage>,
    pub since_id: MessageId,
    pub shown_memory_ids: HashSet<Uuid>,
    pub shown_topic_ids: HashSet<Uuid>,
    /// 章节上下文元信息（tokens/turn_count/step_count）。
    pub context_meta: ContextMeta,
}
