use tokio::time::Instant;

use super::wake::WakeEvent;

/// 事件批处理缓冲
///
/// 收集事件并在窗口期内批处理触发：
///   · rapid 事件 → 立即触发
///   · 普通事件 → 每次新事件滚动 deadline，上限为 first_at + max_duration
pub struct EventBatch {
    events: Vec<WakeEvent>,
    has_rapid: bool,
    first_at: Instant,
}

impl EventBatch {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            has_rapid: false,
            first_at: Instant::now(),
        }
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn push(&mut self, event: WakeEvent) {
        if self.events.is_empty() {
            self.first_at = Instant::now();
        }
        if event.reason.is_rapid() {
            self.has_rapid = true;
        }
        self.events.push(event);
    }

    pub fn flush(&mut self) -> Vec<WakeEvent> {
        self.has_rapid = false;
        std::mem::take(&mut self.events)
    }
}
