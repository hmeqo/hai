pub mod render;

use std::sync::Arc;

use crate::config::{
    AppConfig,
    schema::{PersonalityConfig, TriggerConfig},
};

fn curve(t: f64) -> f64 {
    1.0 - (1.0 - t).powf(1.5)
}

#[derive(Debug, Clone)]
pub struct PersonalityMgr {
    config: Arc<AppConfig>,
}

impl PersonalityMgr {
    pub fn new(config: Arc<AppConfig>) -> Self {
        Self { config }
    }

    pub fn sociability(&self) -> f64 {
        self.config.agent.personality.sociability
    }

    pub fn base_attention(&self, trigger_cfg: &TriggerConfig) -> f64 {
        let t = self.sociability();
        let cap = trigger_cfg.base_attention_cap;
        let base = curve(t) * cap;
        base.max(trigger_cfg.base_attention)
    }

    /// 注意力窗口时长（秒）：agent 主动参与后保持高注意力的时长
    pub fn attention_window_secs(&self) -> f64 {
        let t = self.sociability();
        5.0 + curve(t) * 55.0
    }

    pub fn config(&self) -> PersonalityConfig {
        self.config.agent.personality.clone()
    }
}
