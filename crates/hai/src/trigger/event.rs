use strum::{EnumString, IntoStaticStr};
use uuid::Uuid;

/// 定时/后台任务的具体负载
/// 描述这次 Cron 触发具体是什么任务，让 Agent 能做出合适的响应
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskPayload {
    /// 可选的任务 ID，用于追踪特定后台任务
    pub task_id: Option<Uuid>,
    /// 任务的自然语言描述，告知 Agent 这次触发要做什么
    /// 例如：「用户设置了12点整通知」、「向量索引重建完成」
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

/// 触发 Agent 的原因
///
/// 只描述"是什么触发了 Agent"，**不包含行为指令**。
/// Agent 应当根据自身人格和场景规则自行决定如何响应。
#[derive(Debug, Clone, Default, PartialEq, Eq, EnumString, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum TriggerReason {
    #[default]
    /// 私聊消息触发
    Private,
    /// 被 @ 提及触发
    Mention,
    /// 群聊动态触发机制触发（热度/随机概率）
    /// 用于节省 Token：Agent 被唤醒后自行判断是否需要回复或做后台整理
    Random,
    /// 定时/后台任务触发
    /// 具体的任务语义由 payload 描述，Agent 根据 payload 决定行为
    /// 例如：用户设置的提醒通知、后台任务完成回调等
    Cron(TaskPayload),
    /// 命令触发（用户显式发出指令）
    Command,
}

impl TriggerReason {
    /// 是否应该绕过防抖立即触发
    pub fn should_bypass_debounce(&self) -> bool {
        matches!(self, Self::Cron(_) | Self::Command | Self::Mention)
    }

    pub fn label(&self) -> &'static str {
        self.into()
    }

    /// 返回触发原因的自然语言描述（纯描述性，无行为指令）
    pub fn describe(&self) -> String {
        match self {
            Self::Private => "用户在私聊中发送了消息，请回复。\
                本次触发主要是: 阅读消息、整理消息、维护和更新话题、记忆。".into(),
            Self::Mention => "你在群聊中被 @ 提及。\
                本次触发主要是: 阅读消息、整理消息、维护和更新话题、记忆。".into(),
            Self::Random => "群聊动态触发机制触发了本次运行。\
                本次触发主要是: 阅读消息、整理消息、维护和更新话题、记忆。\
                除非群友正在热烈讨论与你高度相关的话题，或者明确需要你的帮助，否则请**务必保持沉默**，不要发送任何消息。"
                .into(),
            Self::Cron(payload) => {
                if let Some(id) = payload.task_id {
                    format!("定时/后台任务触发 [TaskID:{}]：{}。\n请仅执行任务，不要在群聊中发言，除非任务明确要求。", id, payload.description)
                } else {
                    format!("定时/后台任务触发：{}。\n请仅执行任务，不要在群聊中发言，除非任务明确要求。", payload.description)
                }
            }
            Self::Command => "用户发出了显式指令，请执行并回复结果。".into(),
        }
    }
}

/// Bot → Agent 的事件
#[derive(Debug)]
pub enum AgentEvent {
    /// 消息触发事件
    Message { chat_id: i64, reason: TriggerReason },
}

impl AgentEvent {
    pub fn chat_id(&self) -> i64 {
        match self {
            AgentEvent::Message { chat_id, .. } => *chat_id,
        }
    }

    pub fn reason(&self) -> &TriggerReason {
        match self {
            AgentEvent::Message { reason, .. } => reason,
        }
    }
}

/// Agent → Bot 的信号
#[derive(Debug)]
pub enum BotSignal {
    SendMessage {
        chat_id: i64,
        content: String,
        topic_id: Option<Uuid>,
        /// 平台侧消息 ID，用于回复特定消息
        reply_to_platform_id: Option<String>,
    },
}
