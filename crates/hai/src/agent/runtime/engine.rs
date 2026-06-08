use std::sync::Arc;

use autoagents::{core::agent::DirectAgentHandle, llm::LLMProvider, prelude::*};
use autoagents_toolkit::mcp::{McpConfig, McpServerConfig, McpTools};
use derive_more::Deref;
use tokio::sync::RwLock;

use crate::{
    agent::{
        MainAgent, MainAgentOutput,
        personality::render::personality_context,
        prompts::SYSTEM_PROMPT,
        runtime::ctx::RoundCtx,
        tools::{get_main_agent_tools, skills::load_skill_tool},
    },
    agentcore::{provider::LlmBuildConfig, skills::SkillManager},
    app::AppContext,
    config::AppConfig,
    domain::entity::ChatType,
    error::{AppResultExt, ErrorKind, Result},
};

/// Agent 引擎。负责 LLM 管理、Agent 组装。
#[derive(Clone, Deref)]
pub struct AgentEngine(Arc<AgentEngineInner>);

pub struct AgentEngineInner {
    pub app: AppContext,
    llm: RwLock<Arc<dyn LLMProvider>>,
    pub mcp_tools: McpTools,
    pub skill_manager: SkillManager,
}

impl AgentEngine {
    pub async fn new(app: AppContext) -> Result<Self> {
        let config = Arc::clone(&app.cfg);
        let mcp_tools = Self::load_mcp_tools(&config).await?;
        let skill_manager =
            SkillManager::load(&config.skills.dirs, &config.skills.disabled).await?;
        let llm = Self::build_llm(&app)?;

        Ok(Self(Arc::new(AgentEngineInner {
            app,
            llm: RwLock::new(llm),
            mcp_tools,
            skill_manager,
        })))
    }

    // ── LLM ──

    pub async fn main_llm(&self) -> Arc<dyn LLMProvider> {
        self.llm.read().await.clone()
    }

    fn build_llm(app: &AppContext) -> Result<Arc<dyn LLMProvider>> {
        let provider = app.provider.get_checked(&app.cfg.agent.provider)?;
        let agent_config = &app.cfg.agent;
        let effort = agent_config.reasoning_effort()?;

        let build_cfg = LlmBuildConfig {
            api_key: provider.config.api_key.clone(),
            base_url: provider.base_url.clone(),
            model: agent_config.model.clone(),
            reasoning: agent_config.reasoning,
            reasoning_effort: effort,
            temperature: agent_config.temperature,
            max_tokens: agent_config.max_tokens,
        };

        provider.backend.build(build_cfg)
    }

    // ── Agent 组装 ──

    pub async fn build_handle(
        &self,
        ctx: &RoundCtx,
    ) -> Result<DirectAgentHandle<ReActAgent<MainAgent>>> {
        let mut tools = get_main_agent_tools(ctx);
        tools.extend(self.mcp_tools.get_tools().await);
        tools.extend(load_skill_tool(self.skill_manager.clone()));

        AgentBuilder::<_, DirectAgent>::new(ReActAgent::new(MainAgent {
            tools,
            system_prompt: self.build_system_prompt(ctx.chat_type),
        }))
        .llm(self.main_llm().await)
        .memory(Box::new(SlidingWindowMemory::new(
            self.app.cfg.agent.context.sliding_window_size,
        )))
        .build()
        .await
        .err_kind_msg(ErrorKind::Internal, "Agent builder failed")
    }

    // ── 执行 ──

    pub async fn run(&self, ctx: &RoundCtx, prompt: String) -> Result<MainAgentOutput> {
        let handle = self.build_handle(ctx).await?;
        handle
            .agent
            .run(Task::new(prompt))
            .await
            .err_kind_msg(ErrorKind::Internal, "Agent execution failed")
    }

    // ── System Prompt ──

    pub fn build_system_prompt(&self, chat_type: ChatType) -> String {
        let config = &self.app.cfg;
        let personality_prompt = personality_context(&self.app.agent.personality);

        let mut prompt = SYSTEM_PROMPT.to_owned();
        prompt.push_str("\n\n");
        prompt.push_str(&personality_prompt);

        if !config.agent.context.system_prompt.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(&config.agent.context.system_prompt);
        }
        let extra = if chat_type == ChatType::Private {
            &config.agent.context.private_prompt
        } else {
            &config.agent.context.group_prompt
        };
        if !extra.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(extra);
        }

        if let Some(skills_prompt) = self.skill_manager.discovery_prompt() {
            prompt.push_str("\n\n");
            prompt.push_str(&skills_prompt);
        }

        prompt
    }

    // ── MCP ──

    async fn load_mcp_tools(config: &AppConfig) -> Result<McpTools> {
        McpTools::from_config_object(&McpConfig {
            servers: config
                .mcp
                .iter()
                .map(|(name, mcp)| {
                    let mut cfg =
                        McpServerConfig::new(name.clone(), mcp.r#type.clone(), mcp.command.clone())
                            .with_args(mcp.args.clone());
                    if let Some(env) = &mcp.env {
                        cfg = cfg.with_env(env.clone());
                    }
                    cfg
                })
                .collect(),
        })
        .await
        .err_kind_msg(ErrorKind::Internal, "Failed to load MCP tools")
    }
}
