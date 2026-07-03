use std::sync::{Arc, Mutex};

use genai::chat::ChatMessage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Notify;

use crate::{agent::event::WakeEvent, domain::vo::MessageId};

/// 工具调用的执行结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub tool_name: String,
    pub success: bool,
    pub arguments: Value,
    pub result: Value,
}

impl ToolCallResult {
    pub fn ok(tool_name: impl Into<String>, args: Value, result: Value) -> Self {
        Self {
            tool_name: tool_name.into(),
            success: true,
            arguments: args,
            result,
        }
    }

    pub fn err(tool_name: impl Into<String>, args: Value) -> Self {
        Self {
            tool_name: tool_name.into(),
            success: false,
            arguments: args,
            result: Value::Null,
        }
    }
}

/// 一次 exec_chat 的完整记录。
#[derive(Clone)]
pub struct Turn {
    pub tool_calls: Vec<ToolCallResult>,
    pub response: String,
    pub reasoning: Option<String>,
}

/// CoW 消息容器。沿整条 processing 链路共享，clone = Arc bump。
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

/// Processing 结束后返回给 session 的数据。
pub struct ProcessingOutput {
    pub messages: Messages,
    pub turns: Vec<Turn>,
    pub prompt_tokens: u32,
    pub since_id: MessageId,
    pub has_spoken: bool,
}

// ─── Inbox ────────────────────────────────────────────────────────────────

/// 事件信箱。外部推入、内部取出。
/// Session 持有并下放 clone 给 Processing Task。
#[derive(Clone)]
pub struct Inbox {
    events: Arc<Mutex<Vec<WakeEvent>>>,
    notify: Arc<Notify>,
}

impl Inbox {
    pub fn new() -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
            notify: Arc::new(Notify::new()),
        }
    }

    pub fn push(&self, event: WakeEvent) {
        self.events.lock().unwrap().push(event);
        self.notify.notify_one();
    }

    pub fn drain(&self) -> Vec<WakeEvent> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }

    pub fn notified(&self) -> impl std::future::Future<Output = ()> + '_ {
        self.notify.notified()
    }

    pub fn len(&self) -> usize {
        self.events.lock().unwrap().len()
    }
}
