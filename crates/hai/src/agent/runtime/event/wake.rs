use std::{fmt, sync::Arc};

use derive_more::Deref;
use strum::{EnumString, IntoStaticStr};
use uuid::Uuid;

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
    pub reason: WakeReason,
}

/// 平台 → AgentSession 的一条唤醒通知
#[derive(Debug, Clone, Deref)]
pub struct WakeEvent(Arc<WakeEventInner>);

impl WakeEvent {
    pub fn new(reason: WakeReason) -> Self {
        Self(Arc::new(WakeEventInner { reason }))
    }
}

// ── WakeEvents ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct WakeEvents(Vec<WakeEvent>);

impl WakeEvents {
    pub fn new(events: Vec<WakeEvent>) -> Self {
        Self(events)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.len() == 0
    }

    pub fn iter(&self) -> std::slice::Iter<'_, WakeEvent> {
        self.0.iter()
    }

    pub fn first(&self) -> Option<&WakeEvent> {
        self.0.first()
    }

    pub fn into_vec(self) -> Vec<WakeEvent> {
        self.0
    }

    pub fn extend(&mut self, other: Self) {
        self.0.extend(other.0)
    }

    pub fn take(&mut self) -> Self {
        std::mem::take(self)
    }

    pub fn into_iter(self) -> impl Iterator<Item = WakeEvent> {
        self.0.into_iter()
    }

    pub fn coalesce(&self) -> Vec<EventGroup> {
        let mut groups = Vec::new();
        let mut i = 0;
        while i < self.0.len() {
            let label = self.0[i].reason.label();
            let describe = self.0[i].reason.describe();
            let mut count = 1;
            while i + count < self.0.len() && self.0[i + count].reason == self.0[i].reason {
                count += 1;
            }
            groups.push(EventGroup {
                label,
                describe,
                count,
            });
            i += count;
        }
        groups
    }
}

#[derive(Debug)]
pub struct EventGroup {
    pub label: &'static str,
    pub describe: String,
    pub count: usize,
}

pub trait EventGroupSlice {
    fn reasons_summary(&self) -> String;
}

impl EventGroupSlice for [EventGroup] {
    fn reasons_summary(&self) -> String {
        self.iter().map(|g| g.label).collect::<Vec<_>>().join(", ")
    }
}
