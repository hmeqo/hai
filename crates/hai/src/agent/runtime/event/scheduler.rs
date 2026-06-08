use std::collections::HashSet;

use tokio::time::{Duration, Instant};

use super::{
    attention::{Heat, Window},
    batch::EventBatch,
    wake::{WakeEvent, WakeReason},
};

/// 事件派发的结果
pub enum DispatchResult {
    Ready(Vec<WakeEvent>),
    Wait,
}

// ─── 调度策略 ─────────────────────────────────────────────────────────────────

impl WakeReason {
    pub(super) fn is_addressed(&self) -> bool {
        matches!(self, Self::Direct | Self::Mention)
    }

    pub(super) fn is_rapid(&self) -> bool {
        matches!(self, Self::Scheduled(_) | Self::Command(_))
    }

    pub(super) fn is_mergeable(&self) -> bool {
        matches!(self, Self::Observe | Self::Mention | Self::Direct)
    }
}

// ─── 事件调度器 ───────────────────────────────────────────────────────────────

/// per-session，管理 batch + 热度 + 窗口 + 超时。
pub struct EventScheduler {
    batch: EventBatch,
    heat: Heat,
    window: Window,
}

impl EventScheduler {
    pub fn new(
        base_heat: f64,
        window_secs: f64,
        sustained_window_ms: Duration,
        window_max_ms: Duration,
    ) -> Self {
        Self {
            batch: EventBatch::new(sustained_window_ms, window_max_ms),
            heat: Heat::new(base_heat),
            window: Window::new(window_secs),
        }
    }

    fn refresh_heat(&mut self) {
        self.heat.decay(self.window.closes_at());
    }

    /// 事件入队。被明确指向的事件（Direct/Mention）自动刷新窗口+热量。
    pub fn push(&mut self, event: WakeEvent) {
        self.refresh_heat();

        if event.reason.is_addressed() {
            self.window.refresh();
            self.heat.reset();
        }
        self.batch.push(event);
    }

    /// 尝试派发。注意状态且 batch 非空 → Ready。
    pub fn try_dispatch(&mut self) -> DispatchResult {
        if self.batch.is_empty() {
            return DispatchResult::Wait;
        }

        if self.window.is_active() || rand::random::<f64>() < self.heat.value {
            self.heat.spend();
            return DispatchResult::Ready(self.flush_dedup());
        }

        DispatchResult::Wait
    }

    fn flush_dedup(&mut self) -> Vec<WakeEvent> {
        let events = self.batch.flush();
        let mut seen = HashSet::new();
        let mut items = Vec::new();
        for event in events {
            if event.reason.is_mergeable() {
                if seen.insert(event.reason.label()) {
                    items.push(event);
                }
            } else {
                items.push(event);
            }
        }
        items
    }

    pub fn next_deadline(&self) -> Instant {
        self.batch.next_deadline()
    }

    pub fn is_pending(&self) -> bool {
        !self.batch.is_empty()
    }

    /// 当前调度器快照（供外部查询）
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

    /// 窗口是否激活
    pub fn is_window_active(&self) -> bool {
        self.window.is_active()
    }

    /// 刷新窗口和热量——agent 发送消息后调用
    pub fn refresh(&mut self) {
        self.window.refresh();
        self.heat.reset();
    }

    /// session 是否已过期：窗口关闭后超过 idle_timeout
    pub fn is_expired(&self, idle_timeout: Duration) -> bool {
        let Some(close) = self.window.closes_at() else {
            return false;
        };
        Instant::now() > close + idle_timeout
    }
}

#[derive(Debug)]
pub struct SchedulerStatus {
    pub heat_value: f64,
    pub heat_base: f64,
    pub window_active: bool,
    pub window_closes_in_secs: Option<f64>,
    pub pending_events: usize,
}
