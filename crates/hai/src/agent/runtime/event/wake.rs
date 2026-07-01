use std::{fmt, sync::Arc};

use derive_more::Deref;
use strum::{EnumString, IntoStaticStr};
use uuid::Uuid;

use crate::domain::vo::ChatId;

/// 定时/后台任务的具体负载
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct TaskPayload {
    pub task_id: Option<Uuid>,
    pub description: String,
}

impl TaskPayload {
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            task_id: None,
            description: description.into(),
        }
    }

    pub fn with_id(mut self, task_id: Uuid) -> Self {
        self.task_id = Some(task_id);
        self
    }
}

/// 唤醒原因
///
/// 只描述"为什么唤醒了 Agent"，**不包含行为指令**。
/// Agent 根据自身人格自主决定如何响应。
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, EnumString, IntoStaticStr)]
pub enum WakeReason {
    #[default]
    /// 注意力系统监测到新消息
    Observe,
    /// 有人发来私信
    Direct,
    /// 被 @ 提及
    Mention,
    /// 定时/后台任务
    Scheduled(TaskPayload),
    /// 用户显式指令
    Command(String),
}

impl fmt::Display for WakeReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Observe => write!(f, "observe"),
            Self::Direct => write!(f, "direct"),
            Self::Mention => write!(f, "mention"),
            Self::Scheduled(p) => write!(f, "scheduled({})", p.description),
            Self::Command(c) => write!(f, "command({c})"),
        }
    }
}

impl WakeReason {
    pub fn label(&self) -> &'static str {
        self.into()
    }

    /// 情境描述（纯观察，不含行为指令）
    pub fn describe(&self) -> String {
        match self {
            Self::Direct => "有人发来私信。".to_string(),
            Self::Mention => "有人在群里提到你。".to_string(),
            Self::Observe => "群里有新消息。无主要任务，观察。".to_string(),
            Self::Scheduled(payload) => {
                if let Some(id) = payload.task_id {
                    format!("定时任务 [TaskID:{}]：{}。", id, payload.description)
                } else {
                    format!("定时任务：{}。", payload.description)
                }
            }
            Self::Command(description) => description.clone(),
        }
    }
}

// ─── 调度策略 ─────────────────────────────────────────────────────────────────

impl WakeReason {
    pub fn is_addressed(&self) -> bool {
        matches!(self, Self::Direct | Self::Mention | Self::Command(_))
    }

    pub fn is_rapid(&self) -> bool {
        matches!(self, Self::Scheduled(_) | Self::Command(_))
    }

    pub fn is_mergeable(&self) -> bool {
        matches!(self, Self::Observe | Self::Mention | Self::Direct)
    }
}

/// WakeEvent 的内部数据（通过 Arc 共享）
#[derive(Debug)]
pub struct WakeEventInner {
    pub chat_id: ChatId,
    pub reason: WakeReason,
}

/// 平台 → ChatSession 的一条唤醒通知
#[derive(Debug, Clone, Deref)]
pub struct WakeEvent(Arc<WakeEventInner>);

impl WakeEvent {
    pub fn new(chat_id: ChatId, reason: WakeReason) -> Self {
        Self(Arc::new(WakeEventInner { chat_id, reason }))
    }
}
