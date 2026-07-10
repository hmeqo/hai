use std::sync::Arc;

use derive_more::Deref;
use genai::Client;

use crate::{
    agentcore::{mcp::McpManager, provider, skills::SkillManager},
    app::AppContext,
    error::Result,
};

/// Agent 引擎。全局单例，不持有每轮状态。
/// Deref → AgentEngineInner，session 层直接访问 `self.engine.app.*`。
#[derive(Clone, Deref)]
pub struct AgentEngine(Arc<AgentEngineInner>);

pub struct AgentEngineInner {
    pub app: AppContext,
    pub client: Client,
    pub model: String,
    pub mcp_manager: McpManager,
    pub skill_manager: SkillManager,
}

impl AgentEngine {
    pub async fn new(app: AppContext) -> Result<Self> {
        let cfg = Arc::clone(&app.cfg);
        let mcp_manager = McpManager::from_config(&cfg).await?;
        let skill_manager = SkillManager::load(&cfg.skills.dirs, &cfg.skills.disabled).await?;

        let provider = app.provider.get_checked(&cfg.agent.provider)?;

        let client = provider::create_genai_client(provider)?;

        let model = provider::genai_model_name(provider, &cfg.agent.model);

        Ok(Self(Arc::new(AgentEngineInner {
            app,
            client,
            model,
            mcp_manager,
            skill_manager,
        })))
    }
}
