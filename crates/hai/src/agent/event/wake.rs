use strum::{EnumString, IntoStaticStr};
use uuid::Uuid;

/// 定时/后台任务的具体负载
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskPayload {
    pub task_id: Option<Uuid>,
    pub description: String,
}

impl TaskPayload {
    pub fn new(description: impl Into<String>) -> Self {
        Self { task_id: None, description: description.into() }
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
#[derive(Debug, Clone, Default, PartialEq, Eq, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum WakeReason {
    #[default]
    /// 有人发来私信
    Direct,
    /// 被 @ 提及
    Mention,
    /// 注意力系统判定值得看一眼
    Observe,
    /// 定时/后台任务
    Scheduled(TaskPayload),
    /// 用户显式指令
    Command(String),
}

impl WakeReason {
    /// 是否应绕过防抖立即处理
    pub fn is_rapid(&self) -> bool {
        matches!(self, Self::Scheduled(_) | Self::Command(_))
    }

    /// 当前轮次是否可被新事件打断
    pub fn is_interruptible(&self) -> bool {
        matches!(self, Self::Observe | Self::Mention | Self::Direct)
    }

    pub fn label(&self) -> &'static str {
        self.into()
    }

    /// 情境描述（纯观察，不含行为指令）
    ///
    /// Observe 返回空——没有情境就是最好的情境，agent 自己看消息就知道了。
    pub fn describe(&self) -> String {
        match self {
            Self::Direct => "有人发来私信。".to_string(),
            Self::Mention => "有人在群里提到你。".to_string(),
            Self::Observe => String::new(),
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

    /// 是否可合并（同类无负载信息的唤醒可合并为一条）
    pub fn is_mergeable(&self) -> bool {
        matches!(self, Self::Observe | Self::Mention | Self::Direct)
    }
}

/// 唤醒事件——attention → gateway 的一次通知
#[derive(Debug)]
pub struct WakeEvent {
    pub chat_id: i64,
    pub reason: WakeReason,
}
