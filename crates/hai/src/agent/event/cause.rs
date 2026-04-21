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
pub enum TriggerCause {
    #[default]
    /// 私聊消息触发
    Private,
    /// 被 @ 提及触发
    Mention,
    /// 群聊概率触发（热度/概率）
    /// Agent 随手刷到消息，自行判断是否需要插话或做后台整理，默认不说话
    Random,
    /// 定时/后台任务触发
    /// 具体的任务语义由 payload 描述，Agent 根据 payload 决定行为
    /// 例如：用户设置的提醒通知、后台任务完成回调等
    Cron(TaskPayload),
    /// 命令触发（用户显式发出指令）
    Command(String),
}

impl TriggerCause {
    /// 是否应该绕过防抖立即触发
    pub fn is_rapid(&self) -> bool {
        matches!(self, Self::Cron(_) | Self::Command(_))
    }

    /// 是否可被中断
    pub fn is_interruptible(&self) -> bool {
        matches!(self, Self::Random | Self::Mention | Self::Private)
    }

    pub fn label(&self) -> &'static str {
        self.into()
    }

    /// 返回触发原因的自然语言描述（纯情境描述，不含行为指令）
    pub fn describe(&self) -> String {
        match self {
            Self::Private => "你收到了一条私信。".to_string(),
            Self::Mention => "你在群里被 @ 了。".to_string(),
            Self::Random => "你闲着没事翻了一眼群消息。".to_string(),
            Self::Cron(payload) => {
                if let Some(id) = payload.task_id {
                    format!(
                        "定时/后台任务触发 [TaskID:{}]：{}。\n请仅执行任务。",
                        id, payload.description
                    )
                } else {
                    format!(
                        "定时/后台任务触发：{}。\n请仅执行任务。",
                        payload.description
                    )
                }
            }
            Self::Command(description) => {
                format!("用户发出了显式指令：{}", description)
            }
        }
    }

    /// 是否可与其他同类事件合并（去重）
    ///
    /// 返回 `true` 的事件不携带具体负载信息，多个同类事件语义相同，
    /// 在情景汇总时可合并为一条。例如 3 个 Random 只需保留一个情境描述。
    ///
    /// 返回 `false` 的事件各自携带独立信息（指令内容/任务描述），
    /// 不可合并，必须逐条保留。例如 3 个 Command 需全部展示。
    pub fn is_mergeable(&self) -> bool {
        matches!(self, Self::Random | Self::Mention | Self::Private)
    }
}

/// Bot → Agent 的事件
#[derive(Debug)]
pub enum AgentEvent {
    /// 消息触发事件
    Message { chat_id: i64, cause: TriggerCause },
}

impl AgentEvent {
    pub fn chat_id(&self) -> i64 {
        match self {
            AgentEvent::Message { chat_id, .. } => *chat_id,
        }
    }

    pub fn cause(&self) -> &TriggerCause {
        match self {
            AgentEvent::Message { cause, .. } => cause,
        }
    }
}

/// `[AgentEvent]` 切片上的语义化查询
pub trait AgentEvents {
    fn causes(&self) -> impl Iterator<Item = &TriggerCause>;
    fn all_interruptible(&self) -> bool;
    fn has_private(&self) -> bool;
}

impl AgentEvents for [AgentEvent] {
    fn causes(&self) -> impl Iterator<Item = &TriggerCause> {
        self.iter().map(|e| e.cause())
    }

    fn all_interruptible(&self) -> bool {
        self.causes().all(|c| c.is_interruptible())
    }

    fn has_private(&self) -> bool {
        self.causes().any(|c| matches!(c, TriggerCause::Private))
    }
}

impl AgentEvents for [&AgentEvent] {
    fn causes(&self) -> impl Iterator<Item = &TriggerCause> {
        self.iter().map(|e| e.cause())
    }

    fn all_interruptible(&self) -> bool {
        self.causes().all(|c| c.is_interruptible())
    }

    fn has_private(&self) -> bool {
        self.causes().any(|c| matches!(c, TriggerCause::Private))
    }
}

/// Agent → Bot 的信号
#[derive(Debug)]
pub enum BotSignal {
    SendMessage {
        chat_id: i64,
        content: String,
        topic_id: Option<Uuid>,
        /// 内部消息 ID，用于平台侧回复特定消息
        platform_reply_to_id: Option<i64>,
    },
    /// 发送"正在输入"状态提示
    Typing { chat_id: i64 },
}
