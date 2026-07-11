use std::sync::Arc;

use tokio::sync::Mutex;

use super::shell::ShellRuntime;
use crate::{
    agent::{event::WakeEvents, link::PlatformHandler, multimodal::MultimodalService},
    agentcore::skills::SkillManager,
    app::AppContext,
    domain::{model::ChatType, service::DbServices, vo::ChatId},
};

/// 一轮 task 的完整执行上下文（prompt 构建 + 工具执行共用）
pub struct RunContext {
    pub app: AppContext,
    pub chat_id: ChatId,
    pub chat_type: ChatType,
    pub handler: Arc<dyn PlatformHandler>,
    pub events: WakeEvents,
    pub skill_manager: SkillManager,
    pub db: DbServices,
    pub shell: Arc<Mutex<ShellRuntime>>,
    pub multimodal: MultimodalService,
    pub enabled_parsers: Vec<&'static str>,
    pub tts_enabled: bool,
}

impl RunContext {
    /// `ToolContext` 工厂：从完整上下文中提取工具层字段。
    pub fn tool_ctx(&self) -> ToolContext {
        ToolContext {
            chat_id: self.chat_id,
            handler: self.handler.clone(),
            db: self.db.clone(),
            shell: self.shell.clone(),
            skill_manager: self.skill_manager.clone(),
            multimodal: self.multimodal.clone(),
            enabled_parsers: self.enabled_parsers.clone(),
            tts_enabled: self.tts_enabled,
            sandbox_enabled: self.app.cfg.sandbox.enabled,
            sandbox_image: Some(self.app.cfg.sandbox.image.clone()),
        }
    }
}

/// 工具层窄上下文（不含 `AppContext`）。
pub struct ToolContext {
    pub chat_id: ChatId,
    pub handler: Arc<dyn PlatformHandler>,
    pub db: DbServices,
    pub shell: Arc<Mutex<ShellRuntime>>,
    pub skill_manager: SkillManager,
    pub multimodal: MultimodalService,
    pub enabled_parsers: Vec<&'static str>,
    pub tts_enabled: bool,
    pub sandbox_enabled: bool,
    pub sandbox_image: Option<String>,
}
