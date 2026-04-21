//! 人格 → Prompt 生成器
//!
//! 将人格参数以"数值+含义"的方式传递给 agent，
//! 由 agent 自行判断在具体情境下应如何表现，而非硬编码行为规则。

use crate::config::{
    AppConfigManager,
    schema::{PersonalityConfig, TriggerConfig},
};

fn curve(t: f64) -> f64 {
    1.0 - (1.0 - t).powf(1.5)
}

#[derive(Debug, Clone)]
pub struct PersonalityMgr {
    config: AppConfigManager,
}

impl PersonalityMgr {
    pub fn new(config: AppConfigManager) -> Self {
        Self { config }
    }

    pub fn sociability(&self) -> f64 {
        self.config.load().agent.personality.sociability
    }

    pub fn min_heat(&self, trigger_cfg: &TriggerConfig) -> f64 {
        let t = self.sociability();
        let cap = trigger_cfg.min_heat_cap;
        let min_heat = curve(t) * cap;
        min_heat.max(trigger_cfg.min_heat)
    }

    pub fn conversation_window_secs(&self) -> f64 {
        let t = self.sociability();
        5.0 + curve(t) * 55.0
    }

    pub fn config(&self) -> PersonalityConfig {
        self.config.load().agent.personality.clone()
    }
}
