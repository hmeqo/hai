use std::sync::Arc;

use tokio::sync::Mutex;

use super::shell::ShellRuntime;
use crate::{
    agent::{event::WakeEvent, link::BotHandle, node::MultimodalService},
    agentcore::skills::SkillManager,
    app::AppContext,
    config::schema::SandboxConfig,
    domain::{model::ChatType, service::DbServices, vo::ChatId},
};

/// 一轮 task 的完整执行上下文（prompt 构建 + 工具执行共用）
pub struct RoundContext {
    pub app: AppContext,
    pub chat_id: ChatId,
    pub chat_type: ChatType,
    pub bot: BotHandle,
    pub events: Vec<WakeEvent>,
    pub skill_manager: SkillManager,
    pub db: DbServices,
    pub shell: Arc<Mutex<ShellRuntime>>,
    pub multimodal: MultimodalService,
    pub sandbox: SandboxConfig,
    pub enabled_parsers: Vec<&'static str>,
    pub tts_enabled: bool,
}
