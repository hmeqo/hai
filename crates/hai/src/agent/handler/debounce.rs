use tokio::time::{Duration, Instant};

use crate::agent::event::AgentEvent;

/// 防抖窗口期上限：从第一个事件入队起，最多滚动此长度后强制触发。
/// 同时也是普通事件可打断正在运行任务的时间边界——窗口期外不再打断。
pub const DEBOUNCE_MAX: Duration = Duration::from_secs(5);

/// 防抖队列
///
/// 窗口期（从第一个事件入队起持续 DEBOUNCE_MAX）：
///   · bypass（rapid）事件 → deadline = now，立即触发
///   · 普通事件 → 每次新事件将 deadline 滚动至 now + min_wait，
///     上限为窗口期截止时间（first_at + DEBOUNCE_MAX）
///   · 队列中含 bypass 事件时，后续普通事件也沿用立即触发
pub struct Debouncer {
    events: Vec<AgentEvent>,
    has_rapid: bool,
    /// 首个事件的入队时刻，窗口期基准
    first_at: Instant,
    min_wait: Duration,
}

impl Debouncer {
    pub fn new(min_wait: Duration) -> Self {
        Self {
            events: Vec::new(),
            has_rapid: false,
            first_at: Instant::now(),
            min_wait,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 当前是否仍处于防抖窗口期内（基准为第一个事件入队时刻）
    pub fn is_within_window(&self) -> bool {
        !self.events.is_empty() && self.first_at.elapsed() < DEBOUNCE_MAX
    }

    pub fn push(&mut self, event: AgentEvent) {
        if self.events.is_empty() {
            self.first_at = Instant::now();
        }
        if event.cause().is_rapid() {
            self.has_rapid = true;
        }
        self.events.push(event);
    }

    /// 取出全部积压事件并重置状态
    pub fn flush(&mut self) -> Vec<AgentEvent> {
        self.has_rapid = false;
        std::mem::take(&mut self.events)
    }

    /// 计算下一个触发 deadline
    pub fn next_deadline(&self) -> Instant {
        if self.has_rapid {
            Instant::now()
        } else {
            let rolling = Instant::now() + self.min_wait;
            let hard_cap = self.first_at + DEBOUNCE_MAX;
            rolling.min(hard_cap)
        }
    }
}
