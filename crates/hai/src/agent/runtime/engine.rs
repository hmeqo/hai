use std::sync::Arc;

use derive_more::Deref;
use genai::Client;

use crate::{
    agent::{node::MainAgent, system_prompt::SystemPromptBuilder},
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

        let client = provider::create_genai_client(&provider.config)?;

        let model = provider::genai_model_name(&provider.backend, &cfg.agent.model);

        Ok(Self(Arc::new(AgentEngineInner {
            app,
            client,
            model,
            mcp_manager,
            skill_manager,
        })))
    }

    /// 根据 chat_type 构建 MainAgent（每轮调用）。
    /// 不缓存节点，每次 build 都重新组装 system prompt。
    pub fn build_node(&self, chat_type: crate::domain::model::ChatType) -> MainAgent {
        let cfg = &self.0.app.cfg.agent;
        let max_turns = cfg.context.max_turns;
        let system_prompt = SystemPromptBuilder::new()
            .personality(cfg)
            .system_prompt(cfg)
            .chat_type(cfg, chat_type)
            .skills(&self.0.skill_manager)
            .build();

        MainAgent::new(
            self.0.client.clone(),
            self.0.model.clone(),
            system_prompt,
            max_turns,
        )
    }
}
