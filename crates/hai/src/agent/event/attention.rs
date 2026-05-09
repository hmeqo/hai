use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use super::WakeReason;

// ─── 注意力事件 ──────────────────────────────────────────────────────────────

/// 注意力关心的事件类型
pub enum AttentionEvent {
    /// 有人说话了。`mentioned` 表示是否提到了当前方。
    Message { mentioned: bool },
    /// 当前方参与了聊天（发了消息等）
    Participation,
}

// ─── 参数 ─────────────────────────────────────────────────────────────────────

/// 默认注意力窗口时长（秒），可由 personality.attention_window_secs 覆盖
const DEFAULT_ATTENTION_WINDOW_SECS: f64 = 30.0;

/// 阶梯衰减间隔（秒）：每经过这么长时间，溢出热度砍半一次（二分阶段衰减）
const DECAY_STEP_SECS: f64 = 60.0;

/// 最大热度
const MAX_HEAT: f64 = 1.0;

/// 每次检测消耗的热度
const CONSUME_HEAT: f64 = 0.25;

// ─── 状态 ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct ChatAttention {
    /// 当前热度（概率）
    heat: f64,
    /// 基础注意力（idle 时 heat 衰减的下限，由人格 sociability 决定）
    base_attention: f64,
    /// 注意力窗口起始时间
    window_start_time: Option<Instant>,
    /// 注意力窗口时长（由 personality.attention_window_secs 决定）
    window_duration_secs: f64,
    /// 上次计算衰减的时间锚点（用于阶梯式计算）
    last_decay_anchor: Instant,
}

impl Default for ChatAttention {
    fn default() -> Self {
        Self {
            heat: 0.10,
            base_attention: 0.05,
            window_start_time: None,
            window_duration_secs: DEFAULT_ATTENTION_WINDOW_SECS,
            last_decay_anchor: Instant::now(),
        }
    }
}

impl ChatAttention {
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

        // 【核心：阶段二分计算】向下取整，算出经历了几个完整的"衰减周期"
        let steps = (elapsed / DECAY_STEP_SECS).floor() as i32;

        if steps > 0 {
            let excess_heat = self.heat - self.base_attention;
            if excess_heat > 0.0 {
                self.heat = self.base_attention + excess_heat * 0.5_f64.powi(steps);
            }

            // 推进时间锚点：只推进整数个周期的时长，保留不足一个周期的"零头时间"
            // 这样哪怕零碎发言，满 60 秒依然会准确触发衰减
            self.last_decay_anchor =
                decay_start + Duration::from_secs_f64(steps as f64 * DECAY_STEP_SECS);
        }
    }

    /// 判断当前是否仍在注意力窗口期内
    fn is_in_window(&self) -> bool {
        self.window_start_time
            .map(|t| t.elapsed().as_secs_f64() < self.window_duration_secs)
            .unwrap_or(false)
    }

    /// 收到用户消息时判定（完全靠时间衰减）
    fn invoke(&mut self) -> Option<WakeReason> {
        self.apply_step_decay();

        if self.is_in_window() {
            // 窗口期内：保持实时注意力，100% 唤醒
            self.consume_heat();
            Some(WakeReason::Observe)
        } else if rand::random::<f64>() < self.heat {
            // 窗口期外：概率触发，随手一瞥
            self.consume_heat();
            Some(WakeReason::Observe)
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
        self.heat = (self.heat - CONSUME_HEAT).max(self.base_attention);
    }

    fn status(&self) -> AttentionStatus {
        let is_in_window = self.is_in_window();
        let window_start_time = self.window_start_time;
        let window_elapsed_secs =
            window_start_time.map_or(self.window_duration_secs, |t| t.elapsed().as_secs_f64());
        let window_remaining_secs = if is_in_window {
            (self.window_duration_secs - window_elapsed_secs).max(0.0)
        } else {
            0.0
        };
        AttentionStatus {
            is_in_window,
            window_start_time,
            window_elapsed_secs,
            window_remaining_secs,
            heat: self.heat,
        }
    }
}

pub struct AttentionStatus {
    pub is_in_window: bool,
    pub window_start_time: Option<Instant>,
    pub window_elapsed_secs: f64,
    pub window_remaining_secs: f64,
    pub heat: f64,
}

// ─── 公开接口 ─────────────────────────────────────────────────────────────────

/// 注意力管理器
///
/// 追踪每个 chat 的注意力热度，决定是否值得唤醒 agent 查看新消息。
/// 与 system prompt 正交——唤醒后 agent 完全自主决定是否说话。
pub struct AttentionManager {
    chats: Mutex<HashMap<i64, ChatAttention>>,
    base_attention: f64,
    /// 注意力窗口时长（秒），由 personality.attention_window_secs 决定
    attention_window_secs: f64,
}

impl AttentionManager {
    pub fn new() -> Self {
        Self {
            chats: Mutex::new(HashMap::new()),
            base_attention: 0.1,
            attention_window_secs: DEFAULT_ATTENTION_WINDOW_SECS,
        }
    }

    pub fn with_base_attention(mut self, base: f64) -> Self {
        self.base_attention = base;
        self
    }

    pub fn with_attention_window_secs(mut self, secs: f64) -> Self {
        self.attention_window_secs = secs;
        self
    }

    fn get_or_init_state<'a>(
        &self,
        chats: &'a mut HashMap<i64, ChatAttention>,
        chat_id: i64,
    ) -> &'a mut ChatAttention {
        chats.entry(chat_id).or_insert_with(|| ChatAttention {
            base_attention: self.base_attention,
            heat: self.base_attention,
            window_duration_secs: self.attention_window_secs,
            ..Default::default()
        })
    }

    /// 返回 `Some(WakeReason)` 表示应该唤醒 agent，`None` 表示不值得关注。
    pub fn on_event(&self, chat_id: i64, event: AttentionEvent) -> Option<WakeReason> {
        let mut chats = self.chats.lock().unwrap();
        let state = self.get_or_init_state(&mut chats, chat_id);
        match event {
            AttentionEvent::Message { mentioned } => {
                if mentioned {
                    state.reset_window_with_heat();
                    Some(WakeReason::Mention)
                } else {
                    state.invoke()
                }
            }
            AttentionEvent::Participation => {
                state.reset_window_with_heat();
                None
            }
        }
    }

    pub fn status(&self, chat_id: i64) -> AttentionStatus {
        let mut chats = self.chats.lock().unwrap();
        let state = self.get_or_init_state(&mut chats, chat_id);
        state.apply_step_decay();
        state.status()
    }
}

impl Default for AttentionManager {
    fn default() -> Self {
        Self::new()
    }
}