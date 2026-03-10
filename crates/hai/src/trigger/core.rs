use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

// 假设项目中有该枚举定义
use crate::trigger::TriggerReason;

// ─── 参数 ─────────────────────────────────────────────────────────────────────

/// 连续对话窗口时长（秒）
const CONVERSATION_WINDOW_SECS: f64 = 30.0;

/// 概率半衰期（秒）：【替换了原有的线性衰减】
/// 超出基础概率的部分，每经过这么长时间会衰减一半。
/// 例如超出部分是 0.8，60秒后变成 0.4，120秒后变成 0.2
const HALF_LIFE_SECS: f64 = 60.0;

/// 基础触发概率 (最小触发概率)
const BASE_PROB: f64 = 0.15;

/// 用户消息后概率减少量（正常情况/窗口期外）
const USER_MSG_DECAY: f64 = 0.10;

/// Agent 消息后概率增加量
const AGENT_MSG_BOOST: f64 = 0.15;

/// 最大触发概率
const MAX_PROB: f64 = 1.0;

/// 最小触发概率
const MIN_PROB: f64 = BASE_PROB;

// ─── 状态 ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct ChatTriggerState {
    /// 当前触发概率
    prob: f64,
    /// 窗口期起始/刷新时间 (由 agent回复 或 被@ 触发)
    window_start_time: Option<Instant>,
    /// 上次状态更新时间（用于计算流逝时间）
    last_updated: Instant,
}

impl Default for ChatTriggerState {
    fn default() -> Self {
        Self {
            prob: BASE_PROB,
            window_start_time: None,
            last_updated: Instant::now(),
        }
    }
}

impl ChatTriggerState {
    /// 处理随时间衰减的概率（使用半衰期算法）
    fn apply_time_decay(&mut self) {
        let now = Instant::now();
        let mut decay_start_time = self.last_updated;

        // 【时间计算优化】：确保只计算“窗口期之外”流逝的时间
        // 如果当前有窗口期记录，计算窗口期的结束时间
        if let Some(start_time) = self.window_start_time {
            let window_end_time = start_time + Duration::from_secs_f64(CONVERSATION_WINDOW_SECS);
            // 衰减只能从 (上次更新时间) 和 (窗口结束时间) 中的较晚者开始
            if window_end_time > decay_start_time {
                decay_start_time = window_end_time;
            }
        }

        // 如果现在的时间还没超过衰减开始时间（说明一直处于窗口期内）
        // 则没有有效衰减时间，直接更新 last_updated 并返回
        if now <= decay_start_time {
            self.last_updated = now;
            return;
        }

        // 计算真正在窗口期外流逝的有效时间
        let effective_elapsed = now.duration_since(decay_start_time).as_secs_f64();

        // 更新 last_updated 为当前时间
        self.last_updated = now;

        // 【需求落实】：应用半衰期指数衰减
        let excess_prob = self.prob - MIN_PROB;
        if excess_prob > 0.0 {
            // 公式：当前超出概率 * (0.5 ^ (流逝时间 / 半衰期))
            let decay_factor = 0.5_f64.powf(effective_elapsed / HALF_LIFE_SECS);
            self.prob = MIN_PROB + excess_prob * decay_factor;
        }
    }

    /// 判断当前是否仍在连续对话窗口期内
    fn is_in_window(&self) -> bool {
        self.window_start_time
            .map(|t| t.elapsed().as_secs_f64() < CONVERSATION_WINDOW_SECS)
            .unwrap_or(false)
    }

    /// 用户发送消息时调用
    fn on_user_message(&mut self) -> Option<TriggerReason> {
        // 1. 先结算之前流逝时间的衰减
        self.apply_time_decay();

        // 2. 根据是否在窗口期内，应用不同程度的用户对话惩罚
        if self.is_in_window() {
            // 【需求落实】：窗口期内负面影响更小，且动态取决于接近窗口结束的程度
            let elapsed_in_window = self.window_start_time.unwrap().elapsed().as_secs_f64();

            // 进度比例 (0.0 -> 1.0)，越接近窗口结束时间，比例越接近 1.0
            let window_progress_ratio =
                (elapsed_in_window / CONVERSATION_WINDOW_SECS).clamp(0.0, 1.0);

            // 动态损失：刚进入窗口期时损失接近 0，快脱离时损失接近正常的 USER_MSG_DECAY
            let dynamic_penalty = USER_MSG_DECAY * window_progress_ratio;
            self.prob = (self.prob - dynamic_penalty).max(MIN_PROB);
        } else {
            // 【需求落实】：脱离窗口期后，对话损失恢复正常的 USER_MSG_DECAY
            self.prob = (self.prob - USER_MSG_DECAY).max(MIN_PROB);
        }

        // 3. 掷骰子决定是否触发
        if rand::random::<f64>() < self.prob {
            Some(TriggerReason::Random)
        } else {
            None
        }
    }

    /// Agent 被触发并发送消息后调用
    fn on_agent_sent(&mut self) {
        self.apply_time_decay();

        // 【需求落实】：Agent发送消息后提升一定概率
        self.prob = (self.prob + AGENT_MSG_BOOST).min(MAX_PROB);

        // 【需求落实】：触发并刷新窗口期
        self.window_start_time = Some(Instant::now());
    }

    /// 用户 @ Agent 时调用
    fn on_mention(&mut self) {
        self.apply_time_decay();

        // 【需求落实】：被 @ 不受概率影响，稳定拉满，作为补偿和高优先级对话的开始
        self.prob = MAX_PROB;

        // 【需求落实】：稳定触发/刷新窗口期
        self.window_start_time = Some(Instant::now());
    }

    /// 获取当前触发概率
    fn get_prob(&self) -> f64 {
        self.prob
    }
}

// ─── 触发器详细状态（用于 /status 命令展示） ─────────────────────────────────

pub struct TriggerStatus {
    pub window_elapsed_secs: u64, // 距窗口开始已流逝时间
    pub is_in_window: bool,
    pub trigger_probability: f64,
}

// ─── 公开接口 ─────────────────────────────────────────────────────────────────

pub struct GroupTrigger {
    chats: Mutex<HashMap<i64, ChatTriggerState>>,
}

impl GroupTrigger {
    pub fn new() -> Self {
        Self {
            chats: Mutex::new(HashMap::new()),
        }
    }

    pub fn on_message(&self, chat_id: i64) -> Option<TriggerReason> {
        let mut chats = self.chats.lock().unwrap();
        let state = chats.entry(chat_id).or_default();
        state.on_user_message()
    }

    pub fn on_mention(&self, chat_id: i64) {
        let mut chats = self.chats.lock().unwrap();
        let state = chats.entry(chat_id).or_default();
        // 更新内部状态（拉满概率并启动窗口期），外部调用方通常会无视此处的返回值强制执行回复
        state.on_mention();
    }

    pub fn on_agent_sent(&self, chat_id: i64) {
        let mut chats = self.chats.lock().unwrap();
        let state = chats.entry(chat_id).or_default();
        state.on_agent_sent();
    }

    pub fn cleanup_inactive(&self, inactive_threshold: Duration) {
        let mut chats = self.chats.lock().unwrap();
        chats.retain(|_, state| {
            state
                .window_start_time
                .map(|t| t.elapsed() < inactive_threshold)
                .unwrap_or(true)
        });
    }

    pub fn status(&self, chat_id: i64) -> TriggerStatus {
        let mut chats = self.chats.lock().unwrap();
        let state = chats.entry(chat_id).or_default();

        // 展示前先计算一次最新衰减以保证数据准确
        state.apply_time_decay();

        let window_elapsed = state
            .window_start_time
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);

        TriggerStatus {
            window_elapsed_secs: window_elapsed,
            is_in_window: state.is_in_window(),
            trigger_probability: state.get_prob(),
        }
    }
}

impl Default for GroupTrigger {
    fn default() -> Self {
        Self::new()
    }
}
