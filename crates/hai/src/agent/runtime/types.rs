use std::sync::Arc;

use genai::chat::ChatMessage;

use crate::domain::vo::Turn;

/// CoW 消息容器。沿整条 run 链路共享，clone = Arc bump。
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

/// Run 结束后返回给 session 的数据。
pub struct RunOutput {
    pub messages: Messages,
    pub turns: Vec<Turn>,
}

/// Busy 态的完成信号：一次 run 或 compact 的结果。
pub(crate) enum BusySignal {
    Run(RunOutput),
    Compact(String),
    Failed,
}
