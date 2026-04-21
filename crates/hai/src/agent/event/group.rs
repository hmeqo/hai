use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use super::TriggerCause;

// ─── 参数 ─────────────────────────────────────────────────────────────────────

/// 默认连续对话窗口时长（秒），可由 personality.conversation_window_secs 覆盖
const DEFAULT_CONVERSATION_WINDOW_SECS: f64 = 30.0;

/// 阶梯衰减间隔（秒）：每经过这么长时间，溢出热度砍半一次（二分阶段衰减）
const DECAY_STEP_SECS: f64 = 60.0;

/// 最大热度
const MAX_HEAT: f64 = 1.0;

/// 每次触发消耗的热度
const CONSUME_HEAT: f64 = 0.25;

// ─── 状态 ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct ChatTriggerState {
    /// 当前热度（概率）
    heat: f64,
    /// 最小热度（由人格 sociability 决定）
    min_heat: f64,
    /// 窗口期起始时间
    window_start_time: Option<Instant>,
    /// 对话窗口时长（由 personality.conversation_window_secs 决定）
    window_duration_secs: f64,
    /// 上次计算衰减的时间锚点（用于阶梯式计算）
    last_decay_anchor: Instant,
}

impl Default for ChatTriggerState {
    fn default() -> Self {
        Self {
            heat: 0.10,
            min_heat: 0.05,
            window_start_time: None,
            window_duration_secs: DEFAULT_CONVERSATION_WINDOW_SECS,
            last_decay_anchor: Instant::now(),
        }
    }
}

impl ChatTriggerState {
    /// 执行阶梯式（二分）时间衰减
    fn apply_step_decay(&mut self) {
        let now = Instant::now();
        let mut decay_start = self.last_decay_anchor;

        // 如果存在窗口期，衰减只能从"窗口期结束的那一刻"开始算
        if let Some(start_time) = self.window_start_time {
            let window_end_time = start_time + Duration::from_secs_f64(self.window_duration_secs);
            if window_end_time > decay_start {
                decay_start = window_end_time;
            }
        }

        // 还在窗口期内，或者还没到衰减开始的时间，更新锚点并返回（防止累积错误）
        if now <= decay_start {
            self.last_decay_anchor = now;
            return;
        }

        let elapsed = now.duration_since(decay_start).as_secs_f64();

        // 【核心：阶段二分计算】向下取整，算出经历了几个完整的“衰减周期”
        let steps = (elapsed / DECAY_STEP_SECS).floor() as i32;

        if steps > 0 {
            let excess_heat = self.heat - self.min_heat;
            if excess_heat > 0.0 {
                // 经历了几次周期，就乘几次 0.5
                self.heat = self.min_heat + excess_heat * 0.5_f64.powi(steps);
            }

            // 推进时间锚点：只推进整数个周期的时长，保留不足一个周期的“零头时间”
            // 这样哪怕零碎发言，满 60 秒依然会准确触发衰减
            self.last_decay_anchor =
                decay_start + Duration::from_secs_f64(steps as f64 * DECAY_STEP_SECS);
        }
    }

    /// 判断当前是否仍在连续对话窗口期内
    fn is_in_window(&self) -> bool {
        self.window_start_time
            .map(|t| t.elapsed().as_secs_f64() < self.window_duration_secs)
            .unwrap_or(false)
    }

    /// 收到用户消息时判定（完全靠时间衰减）
    fn invoke(&mut self) -> Option<TriggerCause> {
        self.apply_step_decay();

        if self.is_in_window() {
            // 窗口期内：保持实时注意力，100% 唤醒
            self.consume_heat();
            Some(TriggerCause::Random)
        } else if rand::random::<f64>() < self.heat {
            // 窗口期外：概率触发，随手一瞥
            self.consume_heat();
            Some(TriggerCause::Random)
        } else {
            None
        }
    }

    /// 重置窗口期并拉满热度（被 @ 或 agent 主动回复时调用）
    fn reset_window_with_heat(&mut self) {
        self.heat = MAX_HEAT;
        self.window_start_time = Some(Instant::now());
        self.last_decay_anchor = Instant::now();
    }

    fn consume_heat(&mut self) {
        self.heat = (self.heat - CONSUME_HEAT).max(self.min_heat);
    }

    fn status(&self) -> TriggerStatus {
        let is_in_window = self.is_in_window();
        let window_start_time = self.window_start_time;
        let window_elapsed_secs =
            window_start_time.map_or(self.window_duration_secs, |t| t.elapsed().as_secs_f64());
        let window_remaining_secs = if is_in_window {
            (self.window_duration_secs - window_elapsed_secs).max(0.0)
        } else {
            0.0
        };
        TriggerStatus {
            is_in_window,
            window_start_time,
            window_elapsed_secs,
            window_remaining_secs,
            heat: self.heat,
        }
    }
}

pub struct TriggerStatus {
    pub is_in_window: bool,
    pub window_start_time: Option<Instant>,
    pub window_elapsed_secs: f64,
    pub window_remaining_secs: f64,
    pub heat: f64,
}

// ─── 公开接口 ─────────────────────────────────────────────────────────────────

pub struct GroupTrigger {
    chats: Mutex<HashMap<i64, ChatTriggerState>>,
    min_heat: f64,
    /// 对话窗口时长（秒），由 personality.conversation_window_secs 决定
    conversation_window_secs: f64,
}

impl GroupTrigger {
    pub fn new() -> Self {
        Self {
            chats: Mutex::new(HashMap::new()),
            min_heat: 0.1,
            conversation_window_secs: DEFAULT_CONVERSATION_WINDOW_SECS,
        }
    }

    pub fn with_min_heat(mut self, min_heat: f64) -> Self {
        self.min_heat = min_heat;
        self
    }

    pub fn with_conversation_window_secs(mut self, secs: f64) -> Self {
        self.conversation_window_secs = secs;
        self
    }

    /// 群消息到达时调用。
    ///
    /// - `is_mention`：消息是否 @ 了 bot。
    ///   - `true`：强制唤醒（热度拉满），返回 `Some(Mention)`
    ///   - `false`：按热度概率决定，返回 `Some(Random)` 或 `None`
    fn get_or_init_state<'a>(
        &self,
        chats: &'a mut HashMap<i64, ChatTriggerState>,
        chat_id: i64,
    ) -> &'a mut ChatTriggerState {
        chats.entry(chat_id).or_insert_with(|| ChatTriggerState {
            min_heat: self.min_heat,
            heat: self.min_heat,
            window_duration_secs: self.conversation_window_secs,
            ..Default::default()
        })
    }

    pub fn on_message(&self, chat_id: i64, is_mention: bool) -> Option<TriggerCause> {
        let mut chats = self.chats.lock().unwrap();
        let state = self.get_or_init_state(&mut chats, chat_id);
        if is_mention {
            state.reset_window_with_heat();
            Some(TriggerCause::Mention)
        } else {
            state.invoke()
        }
    }

    /// Agent 完成回复后调用，拉满热度并开启窗口期保持实时注意力。
    ///
    /// 窗口期内每条消息都会唤醒 agent，但 agent 收到的触发原因与普通概率触发相同，
    /// 由 agent 自己决定是否说话。
    pub fn on_agent_replied(&self, chat_id: i64) {
        let mut chats = self.chats.lock().unwrap();
        let state = self.get_or_init_state(&mut chats, chat_id);
        state.reset_window_with_heat();
    }

    pub fn status(&self, chat_id: i64) -> TriggerStatus {
        let mut chats = self.chats.lock().unwrap();
        let state = self.get_or_init_state(&mut chats, chat_id);
        state.apply_step_decay();
        state.status()
    }
}

impl Default for GroupTrigger {
    fn default() -> Self {
        Self::new()
    }
}
