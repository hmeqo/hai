pub mod debounce;
pub mod gateway;
pub mod session;
pub mod task;

pub use gateway::AgentGateway;

use std::{collections::HashSet, sync::Arc};

use autoagents::{core::agent::DirectAgentHandle, llm::LLMProvider, prelude::*};
use autoagents_toolkit::mcp::{McpConfig, McpServerConfig, McpTools};
use tokio::sync::RwLock;

use crate::{
    agent::{
        MainAgent,
        context::render_main_context,
        event::WakeEvent,
        prompts::TOOL_MANUAL,
        round::RoundContext,
        tools::{get_main_agent_tools, skills::load_skill_tool},
    },
    agentcore::{
        provider::LlmBuildConfig,
        render::{Section, item, section},
        skills::SkillManager,
    },
    app::AppContext,
    config::AppConfig,
    error::{AppResultExt, ErrorKind, Result},
    personality::render::personality_context,
};

/// Agent 执行上下文（可在 Arc 内共享）
///
/// 负责 LLM 管理、Agent 组装、任务执行。不关心事件路由和连接管理。
pub struct AgentCtx {
    pub app: AppContext,
    llm: RwLock<Arc<dyn LLMProvider>>,
    pub mcp_tools: McpTools,
    pub skill_manager: Arc<SkillManager>,
}

impl AgentCtx {
    pub async fn new(app: AppContext) -> Result<Self> {
        let config = Arc::clone(&app.cfg);
        let mcp_tools = Self::load_mcp_tools(&config).await?;
        let skill_manager = Arc::new(SkillManager::load(&config.skills.dirs).await?);
        let llm = Self::build_llm(&app)?;

        Ok(Self { app, llm: RwLock::new(llm), mcp_tools, skill_manager })
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

    pub async fn main_agent_handle(
        &self,
        rc: &RoundContext,
    ) -> Result<DirectAgentHandle<ReActAgent<MainAgent>>> {
        let mut tools = get_main_agent_tools(rc);
        tools.extend(self.mcp_tools.get_tools().await);
        tools.extend(load_skill_tool(Arc::clone(&self.skill_manager)));

        AgentBuilder::<_, DirectAgent>::new(ReActAgent::new(MainAgent {
            tools,
            system_prompt: self.build_system_prompt(),
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

    pub async fn execute(&self, rc: RoundContext) -> Result<()> {
        let chat_id = rc.chat_id;
        let events = preprocess_events(&rc.events);
        let reasons: Vec<&str> = events.iter().map(|e| e.reason.label()).collect();
        tracing::info!(chat_id, reasons = ?reasons, "Agent woke up");

        rc.conn.send_typing(chat_id).await;

        let ctx = self
            .app
            .agent
            .context_fty
            .build_context(
                rc.conn.profile.clone(),
                chat_id,
                self.app.cfg.agent.context.message_history_limit,
            )
            .await?;

        let message_ids: Vec<i64> = ctx.message_ids.clone();
        let task_message = render_main_context(&ctx, build_trigger_section(events));
        tracing::info!(chat_id, "Agent task message:\n{task_message}");

        let response: String = self
            .main_agent_handle(&rc)
            .await?
            .agent
            .run(Task::new(task_message))
            .await
            .err_kind_msg(ErrorKind::Internal, "Agent execution failed")?;

        if !message_ids.is_empty() {
            if let Err(err) = self.app.db.srv.message.mark_unread_seen(&message_ids).await {
                tracing::warn!(chat_id, "Failed to mark messages seen: {err}");
            }
        }

        tracing::info!(chat_id, response, "Agent done");
        Ok(())
    }

    // ── System Prompt ──

    pub fn build_system_prompt(&self) -> String {
        let config = &self.app.cfg;
        let personality_prompt = personality_context(&self.app.agent.personality);

        let mut prompt = personality_prompt;
        prompt.push_str("\n\n");
        prompt.push_str(TOOL_MANUAL);

        if !config.agent.context.system_prompt.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(&config.agent.context.system_prompt);
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

// ─── 内部工具 ─────────────────────────────────────────────────────────────

fn preprocess_events(events: &[WakeEvent]) -> Vec<&WakeEvent> {
    let mut seen = HashSet::new();
    let mut items = Vec::new();

    for event in events {
        let reason = &event.reason;
        if reason.is_mergeable() {
            if seen.insert(reason.label()) {
                items.push(event);
            }
        } else {
            items.push(event);
        }
    }
    items
}

fn build_trigger_section<'a>(events: impl IntoIterator<Item = &'a WakeEvent>) -> Section {
    let mut items = Vec::new();
    for event in events.into_iter() {
        let desc = event.reason.describe();
        if !desc.is_empty() {
            items.push(item("context").with_content(desc));
        }
    }

    if items.is_empty() {
        return section("situation");
    }

    if items.len() == 1 {
        section("situation").add_child(items.remove(0))
    } else {
        section("situation").add_children(items)
    }
}
