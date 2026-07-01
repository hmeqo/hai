use std::sync::Arc;

use tokio::sync::Mutex;

use crate::{
    agent::{link::BotHandle, node::MultimodalService, runtime::shell::ShellRuntime},
    agentcore::skills::SkillManager,
    domain::{service::DbServices, vo::ChatId},
};

/// 工具层执行上下文：chat_id、bot、db、shell、multimodal 等依赖。
pub struct ToolContext {
    pub chat_id: ChatId,
    pub bot: BotHandle,
    pub db: DbServices,
    pub shell: Arc<Mutex<ShellRuntime>>,
    pub skill_manager: SkillManager,
    pub multimodal: MultimodalService,
    pub enabled_parsers: Vec<&'static str>,
    pub tts_enabled: bool,
    /// 容器 sandbox 是否启用（用于 RunShell 动态描述）
    pub sandbox_enabled: bool,
    /// 容器镜像名（用于 RunShell 动态描述）
    pub sandbox_image: String,
}
