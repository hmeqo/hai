use tokio::time::{Duration, Instant};

use super::attention::{Heat, Window};
use crate::{
    agent::event::{WakeEvent, WakeEvents},
    config::schema::AttentionConfig,
};

const DEBOUNCE_DURATION: Duration = Duration::from_millis(1500);

/// 调度器决策。
pub enum Decision {
    /// 时机成熟，返回待派发的事件。
    Ready(WakeEvents),
    /// 时机未到（debounce / heat 没中），继续等。
    Defer,
    /// Session 闲置超时，该退出了。
    Done,
}

/// 事件调度器——只做时机决策并持有待派发队列。
pub struct EventScheduler {
    queue: WakeEvents,
    heat: Heat,
    window: Window,
    debounce_until: Option<Instant>,
}

impl EventScheduler {
    pub fn new(cfg: &AttentionConfig) -> Self {
        Self {
            queue: WakeEvents::default(),
            heat: Heat::new(cfg.base_attention),
            window: Window::new(cfg.window_secs),
            debounce_until: None,
        }
    }

    fn refresh_heat(&mut self) {
        self.heat.decay(self.window.closes_at());
    }

    fn on_event(&mut self, event: &WakeEvent) {
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
    }

    /// 获取下次应该 poll 的时间。
    pub fn next_deadline(&self, idle_timeout: Duration) -> Option<Instant> {
        if let Some(d) = self.debounce_until {
            return Some(d);
        }
        if let Some(close) = self.window.closes_at() {
            return Some(close + idle_timeout);
        }
        None
    }

    /// 将事件放入队列并更新调度状态。
    pub fn enqueue(&mut self, events: WakeEvents) {
        for event in events.iter() {
            self.on_event(event);
        }
        self.queue.extend(events);
    }

    /// 判断是否可以派发。如果可以则返回 `Ready(events)`。
    pub fn decide(&mut self, idle_timeout: Duration) -> Decision {
        let now = Instant::now();

        if let Some(d) = self.debounce_until {
            if now < d {
                return Decision::Defer;
            }
            self.debounce_until = None;
        }

        self.refresh_heat();

        if self.window.is_active() {
            if self.queue.is_empty() {
                return Decision::Defer;
            }
            return Decision::Ready(self.queue.take());
        }

        if self.heat.value > 0.0 && rand::random::<f64>() < self.heat.value {
            self.heat.spend();
            return Decision::Ready(self.queue.take());
        }

        if let Some(close) = self.window.closes_at()
            && now > close + idle_timeout
        {
            return if self.queue.is_empty() {
                Decision::Done
            } else {
                Decision::Ready(self.queue.take())
            };
        }

        Decision::Defer
    }

    /// 刷新窗口和热量——agent 发送消息后调用
    pub fn refresh(&mut self) {
        self.window.refresh();
        self.heat.reset();
    }

    pub fn snapshot(&mut self) -> SchedulerStatus {
        self.refresh_heat();
        SchedulerStatus {
            heat_value: self.heat.value,
            heat_base: self.heat.base,
            window_active: self.window.is_active(),
            window_closes_in_secs: self.window.closes_in().map(|d| d.as_secs_f64()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SchedulerStatus {
    pub heat_value: f64,
    pub heat_base: f64,
    pub window_active: bool,
    pub window_closes_in_secs: Option<f64>,
}
