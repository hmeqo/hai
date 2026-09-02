use std::{fmt, sync::Arc};

use derive_more::Deref;
use strum::IntoStaticStr;
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

/// 用户显式指令（内置命令）
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AgentCommand {
    OrganizeMemory,
    Explain,
    Digest(u32),
}

impl AgentCommand {
    /// 给 agent 的执行指令文本（describe 注入上下文的表述）
    pub fn instruction(&self) -> String {
        match self {
            Self::OrganizeMemory => {
                "执行记忆/主题整理, 包括不限于处理不符合规范的记忆或主题, 删除重建".into()
            }
            Self::Explain => "向用户解释，客观中立，不偏向任何一方。\
                 若命令带了要解释的对象，解释该对象；\
                 否则从上文最近提到的概念里判断值得解释的内容。\
                 直接给出解释"
                .into(),
            Self::Digest(days) => {
                format!(
                    "客观总结最近 {days} 天内聊过的值得注意的内容给用户：\
                     主动用 search_topics 检索最近 {days} 天这个时间范围内的话题\
                     （必要时配合语义关键词），并浏览相关消息；客观提炼用户可能错过的、\
                     值得关注的重要内容，注明每条的时间，条理清晰地回复"
                )
            }
        }
    }
}

/// 唤醒原因
///
/// 只描述"为什么唤醒了 Agent"，**不包含行为指令**。
/// Agent 根据自身人格自主决定如何响应。
#[derive(Debug, Clone, PartialEq, Eq, Hash, IntoStaticStr)]
pub enum WakeReason {
    /// 注意力系统监测到新消息
    Observe,
    /// 有人发来私信
    Direct,
    /// 被 @ 提及
    Mention,
    /// 定时/后台任务
    Scheduled(TaskPayload),
    /// 用户显式指令
    Command(AgentCommand),
}

impl fmt::Display for WakeReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Observe => write!(f, "observe"),
            Self::Direct => write!(f, "direct"),
            Self::Mention => write!(f, "mention"),
            Self::Scheduled(p) => write!(f, "scheduled({})", p.description),
            Self::Command(c) => write!(f, "command({c:?})"),
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
            Self::Command(cmd) => cmd.instruction(),
        }
    }
}

// ─── 调度策略 ─────────────────────────────────────────────────────────────────

impl WakeReason {
    pub fn is_addressed(&self) -> bool {
        matches!(
            self,
            Self::Direct | Self::Mention | Self::Scheduled(_) | Self::Command(_)
        )
    }

    pub fn is_rapid(&self) -> bool {
        matches!(
            self,
            Self::Direct | Self::Mention | Self::Scheduled(_) | Self::Command(_)
        )
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
