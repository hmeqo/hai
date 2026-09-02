use std::sync::Arc;

use genai::chat::ChatMessage;

use super::event::WakeEvents;
use crate::domain::vo::Step;

/// CoW 消息容器。沿整条 turn 链路共享，clone = Arc bump。
#[derive(Clone)]
pub struct Messages(Arc<Vec<ChatMessage>>);

impl Messages {
    pub fn new(msgs: Vec<ChatMessage>) -> Self {
        Self(Arc::new(msgs))
    }

    pub fn push(&mut self, msg: ChatMessage) {
        Arc::make_mut(&mut self.0).push(msg);
    }

    pub fn extend(&mut self, msgs: impl IntoIterator<Item = ChatMessage>) {
        Arc::make_mut(&mut self.0).extend(msgs);
    }

    pub fn to_vec(&self) -> Vec<ChatMessage> {
        (*self.0).clone()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// Turn 结束后返回给 session 的数据。
pub struct TurnOutput {
    pub messages: Messages,
    pub steps: Vec<Step>,
}

/// Busy 态的完成信号：一次 turn / steering 打断 / 收尾 的结果。
/// 失败变体按来源区分（turn vs 收尾）——事件循环据此决定是否抑制 token 阈值触发。
pub(crate) enum BusySignal {
    Turn(TurnOutput),
    /// 提前正常结束（turn 期间新事件打断，steering）：已处理内容生效，
    /// 附带打断事件（立即增量续跑新 turn）。
    Steered(TurnOutput, WakeEvents),
    WrapUp(String),
    TurnFailed,
    WrapUpFailed(String),
}
