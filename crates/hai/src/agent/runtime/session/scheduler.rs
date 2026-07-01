use tokio::time::{Duration, Instant};

use super::attention::{Heat, Window};
use crate::agent::event::WakeEvent;

const DEBOUNCE_DURATION: Duration = Duration::from_millis(500);

/// 热度不足后的重试间隔。一次 retry 后仍不够则丢弃 batch。
const RETRY_INTERVAL: Duration = Duration::from_secs(30);

/// 调度器轮询结果
pub enum PollOutcome {
    Dispatch(Vec<WakeEvent>),
    Wait,
    Expired,
}

// ─── 事件缓冲 ──────────────────────────────────────────────────────────────────

struct EventBatch {
    events: Vec<WakeEvent>,
    has_rapid: bool,
}

impl EventBatch {
    fn new() -> Self {
        Self {
            events: Vec::new(),
            has_rapid: false,
        }
    }

    fn len(&self) -> usize {
        self.events.len()
    }

    fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    fn has_rapid(&self) -> bool {
        self.has_rapid
    }

    fn push(&mut self, event: WakeEvent) {
        if event.reason.is_rapid() {
            self.has_rapid = true;
        }
        self.events.push(event);
    }

    fn flush(&mut self) -> Vec<WakeEvent> {
        self.has_rapid = false;
        std::mem::take(&mut self.events)
    }

    fn discard(&mut self) {
        self.has_rapid = false;
        self.events.clear();
    }
}

// ─── 事件调度器 ───────────────────────────────────────────────────────────────

/// per-session，管理 batch + 热度 + 窗口 + 防抖 + 单次重试。
pub struct EventScheduler {
    batch: EventBatch,
    heat: Heat,
    window: Window,
    debounce_until: Option<Instant>,
    /// heat 检查失败后的重试时间点。已存在此值时再次失败则丢弃 batch。
    retry_at: Option<Instant>,
}

impl EventScheduler {
    pub fn new(base_heat: f64, window_secs: f64) -> Self {
        Self {
            batch: EventBatch::new(),
            heat: Heat::new(base_heat),
            window: Window::new(window_secs),
            debounce_until: None,
            retry_at: None,
        }
    }

    fn refresh_heat(&mut self) {
        self.heat.decay(self.window.closes_at());
    }

    /// 事件入队。
    pub fn push(&mut self, event: WakeEvent) {
        self.refresh_heat();

        if event.reason.is_addressed() {
            self.window.refresh();
            self.heat.reset();
        }

        if event.reason.is_rapid() {
            self.debounce_until = None;
        } else {
            self.debounce_until = Some(Instant::now() + DEBOUNCE_DURATION);
        }

        // 新事件到达，情况已变化，取消 pending 重试
        self.retry_at = None;

        self.batch.push(event);
    }

    /// 返回下次应该 poll 的时间。
    pub fn next_deadline(&self, idle_timeout: Duration) -> Option<Instant> {
        if let Some(d) = self.debounce_until {
            return Some(d);
        }

        if let Some(r) = self.retry_at {
            return Some(r);
        }

        if let Some(close) = self.window.closes_at() {
            return Some(close + idle_timeout);
        }

        None
    }

    /// 轮询调度器。在 `next_deadline` 返回的 deadline 到来时调用。
    pub fn poll(&mut self, idle_timeout: Duration) -> PollOutcome {
        let now = Instant::now();

        // 防抖尚未过期 → 继续等
        if let Some(d) = self.debounce_until {
            if now < d {
                return PollOutcome::Wait;
            }
            self.debounce_until = None;
        }

        self.refresh_heat();

        // batch 空 → 检查过期
        if self.batch.is_empty() {
            if let Some(close) = self.window.closes_at()
                && now > close + idle_timeout
            {
                return PollOutcome::Expired;
            }
            return PollOutcome::Wait;
        }

        // batch 有事件 → 尝试派发
        if self.batch.has_rapid() {
            return PollOutcome::Dispatch(self.batch.flush());
        }
        if self.window.is_active() {
            return PollOutcome::Dispatch(self.batch.flush());
        }
        if rand::random::<f64>() < self.heat.value {
            self.heat.spend();
            return PollOutcome::Dispatch(self.batch.flush());
        }

        // 热度不够：安排单次重试或丢弃
        if self.retry_at.is_none() {
            self.retry_at = Some(now + RETRY_INTERVAL);
            tracing::debug!(
                heat = %self.heat.value,
                "Heat too low, will retry in 30s",
            );
            PollOutcome::Wait
        } else {
            tracing::debug!(
                heat = %self.heat.value,
                pending = self.batch.len(),
                "Heat still too low after retry, discarding batch",
            );
            self.batch.discard();
            self.retry_at = None;
            PollOutcome::Wait
        }
    }

    pub fn snapshot(&mut self) -> SchedulerStatus {
        self.refresh_heat();
        SchedulerStatus {
            heat_value: self.heat.value,
            heat_base: self.heat.base,
            window_active: self.window.is_active(),
            window_closes_in_secs: self.window.closes_in().map(|d| d.as_secs_f64()),
            pending_events: self.batch.len(),
        }
    }

    /// 刷新窗口和热量——agent 发送消息后调用
    pub fn refresh(&mut self) {
        self.window.refresh();
        self.heat.reset();
    }
}

#[derive(Debug, Clone)]
pub struct SchedulerStatus {
    pub heat_value: f64,
    pub heat_base: f64,
    pub window_active: bool,
    pub window_closes_in_secs: Option<f64>,
    pub pending_events: usize,
}
