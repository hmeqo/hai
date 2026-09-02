use tokio::time::{Duration, Instant};

use super::attention::{Heat, Window};
use crate::{
    agent::event::{WakeEvent, WakeEvents},
    config::schema::AttentionConfig,
};

const DEBOUNCE_DURATION: Duration = Duration::from_millis(1500);

/// 调度器决策。
#[derive(Debug)]
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
    /// 上次活动时刻（任意事件入队 / agent 发言刷新）——idle 计时基准，
    /// 与注意力窗口解耦（无窗口场景也有 idle deadline）。
    last_activity: Option<Instant>,
}

impl EventScheduler {
    pub fn new(cfg: &AttentionConfig) -> Self {
        Self {
            queue: WakeEvents::default(),
            heat: Heat::new(cfg.base_attention),
            window: Window::new(cfg.window_secs),
            debounce_until: None,
            // idle 计时基准从会话创建起算：纯 Observe 会话（无 addressed 刷新）也能
            // idle 到期退出，不会 park 永不退出（Observe 不刷新）
            last_activity: Some(Instant::now()),
        }
    }

    fn refresh_heat(&mut self) {
        self.heat.decay(self.window.closes_at());
    }

    fn on_event(&mut self, event: &WakeEvent) {
        self.refresh_heat();

        if event.reason.is_addressed() {
            // 注意力窗口期活动（发言/被 @/私信/指令）刷新 idle 基准；
            // Observe 不是注意力（纯观察），不刷新。
            self.last_activity = Some(Instant::now());
            self.window.refresh();
            self.heat.reset();
        }

        if event.reason.is_rapid() {
            self.debounce_until = None;
        } else {
            self.debounce_until = Some(Instant::now() + DEBOUNCE_DURATION);
        }
    }

    pub fn next_deadline(&self, idle_timeout: Duration) -> Option<Instant> {
        if let Some(d) = self.debounce_until {
            return Some(d);
        }
        self.idle_base().map(|t| t + idle_timeout)
    }

    /// idle 计时基准 = 窗口关闭时刻（有窗口时，兼容"窗口期结束后 idle_timeout"语义）
    /// 或上次活动时刻（无窗口时——恢复会话/纯 Observe 场景的兜底）。
    fn idle_base(&self) -> Option<Instant> {
        self.window.closes_at().or(self.last_activity)
    }

    /// 入队会重置防抖与热量状态。
    pub fn enqueue(&mut self, events: WakeEvents) {
        for event in events.iter() {
            self.on_event(event);
        }
        self.queue.extend(events);
    }

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

        // heat 随机关注只对"有事件可关注"生效——队列空不返回 Ready(空)（空 turn 修复）
        if !self.queue.is_empty()
            && self.heat.value > 0.0
            && rand::random::<f64>() < self.heat.value
        {
            self.heat.spend();
            return Decision::Ready(self.queue.take());
        }

        if let Some(base) = self.idle_base()
            && now > base + idle_timeout
        {
            // idle 到期恒 Done：不派发积压（Observe 积压由窗口期/heat 正常路径处理，
            // 消息内容由 DB + since_id 游标保底）。
            return Decision::Done;
        }

        Decision::Defer
    }

    /// 发言也是活动：发送消息后刷新 idle 基准。
    pub fn refresh(&mut self) {
        self.window.refresh();
        self.heat.reset();
        self.last_activity = Some(Instant::now());
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::event::{WakeEvent, WakeReason},
        config::schema::AttentionConfig,
    };

    fn cfg(base: f64, window_secs: f64) -> AttentionConfig {
        AttentionConfig {
            base_attention: base,
            window_secs,
        }
    }

    fn observe() -> WakeEvents {
        WakeEvents::new(vec![WakeEvent::new(WakeReason::Observe)])
    }

    /// Observe 入队后 debounce 期间必须有 deadline：Observe 不刷新 last_activity（非注意力），
    /// debounce 是 Observe 场景唯一的短期 deadline，之后由窗口期/heat 正常路径接管。
    #[test]
    fn observe_has_debounce_deadline_without_window() {
        let mut s = EventScheduler::new(&cfg(0.0, 30.0));
        s.enqueue(observe());
        assert!(
            s.next_deadline(Duration::from_secs(300)).is_some(),
            "Observe 入队后 debounce deadline 必须存在"
        );
    }

    /// idle 到期 → 恒 Done（不派发积压：Observe 积压由窗口期/heat 正常路径处理，
    /// 消息内容由 DB + since_id 游标保底）。全程不依赖窗口。
    #[test]
    fn decide_done_on_idle_expiry_even_with_queue() {
        let mut s = EventScheduler::new(&cfg(0.0, 30.0));
        s.enqueue(observe());
        s.debounce_until = None;
        s.last_activity = Some(Instant::now() - Duration::from_secs(301));

        match s.decide(Duration::from_secs(300)) {
            Decision::Done => {}
            other => panic!("idle 到期应恒 Done（含队列非空），实际 {other:?}"),
        }
    }

    /// Observe 不是注意力：入队不刷新 last_activity（addressed 才刷新）。
    #[test]
    fn observe_does_not_refresh_last_activity() {
        let mut s = EventScheduler::new(&cfg(0.0, 30.0));
        s.last_activity = Some(Instant::now() - Duration::from_secs(100));
        let before = s.last_activity;
        s.enqueue(observe());
        assert_eq!(
            s.last_activity, before,
            "Observe 不应刷新 last_activity（非注意力）"
        );
    }

    /// 空 turn 修复：heat 命中但队列空 → 不得返回 Ready(空)。
    #[test]
    fn no_empty_run_on_heat_hit() {
        let mut s = EventScheduler::new(&cfg(0.0, 30.0));
        s.heat.value = 1.0; // 模拟 heat 必中
        match s.decide(Duration::from_secs(300)) {
            Decision::Defer => {}
            other => panic!("heat 命中 + 队列空应 Defer，实际 {other:?}"),
        }
    }
}
