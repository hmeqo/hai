use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::entity::{Memory, MemoryType, Topic};

/// 统一记忆输入参数
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum MemoryInput {
    /// 记录关于用户的客观事实
    /// 作用范围: 特定用户 (account_id) 在特定群聊 (chat_id) 中的信息
    CreateUserFact {
        account_id: i64,
        chat_id: i64,
        content: String,
    },
    /// 更新关于用户的客观事实
    UpdateUserFact {
        id: Uuid,
        content: Option<String>,
        importance: Option<i32>,
    },
    /// 记录 Agent 的笔记
    /// 作用范围: 特定群聊 (chat_id) 下，可选关联话题或消息
    CreateAgentNote {
        chat_id: i64,
        references: Option<serde_json::Value>,
        content: String,
    },
    /// 更新 Agent 的笔记
    UpdateAgentNote {
        id: Uuid,
        content: Option<String>,
        importance: Option<i32>,
    },
    /// 记录通用知识
    /// 作用范围: 特定群聊 (chat_id) 内共享的知识
    CreateKnowledge { chat_id: i64, content: String },
    /// 更新通用知识
    UpdateKnowledge {
        id: Uuid,
        content: Option<String>,
        importance: Option<i32>,
    },
    /// 更新群聊固定上下文（规则/人设）
    /// 作用范围: 特定群聊 (chat_id)，每个群聊仅存一份，重复调用会覆盖
    UpsertChatRule { chat_id: i64, content: String },
}

impl MemoryInput {
    pub fn memory_type(&self) -> MemoryType {
        match self {
            MemoryInput::CreateUserFact { .. } | MemoryInput::UpdateUserFact { .. } => {
                MemoryType::UserFact
            }
            MemoryInput::CreateAgentNote { .. } | MemoryInput::UpdateAgentNote { .. } => {
                MemoryType::AgentNote
            }
            MemoryInput::CreateKnowledge { .. } | MemoryInput::UpdateKnowledge { .. } => {
                MemoryType::Knowledge
            }
            MemoryInput::UpsertChatRule { .. } => MemoryType::Rule,
        }
    }
}

/// 带相似度距离的话题检索结果（pgvector `<=>` 余弦距离，越小越相似）
#[derive(Debug, Clone)]
pub struct TopicSearchResult {
    pub topic: Topic,
    pub distance: f64,
}

/// 带相似度距离的记忆检索结果
#[derive(Debug, Clone)]
pub struct MemorySearchResult {
    pub memory: Memory,
    pub distance: f64,
}
