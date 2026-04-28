mod debounce;
mod session;
mod task;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use autoagents::{core::agent::DirectAgentHandle, llm::LLMProvider, prelude::*};
use autoagents_toolkit::mcp::{McpConfig, McpServerConfig, McpTools};
pub use session::spawn_chat_session;
use tokio::sync::{RwLock, mpsc};

use crate::{
    agent::{
        MainAgent,
        context::render_main_context,
        event::{AgentEvent, AgentEvents, BotSignal},
        prompts::{TOOL_MANUAL, personality_context},
        tools::{ToolContext, get_main_agent_tools, skills::load_skill_tool},
    },
    agentcore::{
        provider::LlmBuildConfig,
        render::{Section, item, section},
        skills::SkillManager,
    },
    app::AppContext,
    config::AppConfig,
    domain::entity::ChatType,
    error::{AppResultExt, ErrorKind, Result},
};

/// 核心 Agent 处理器
///
/// 持有 `AppContext`（所有共享依赖）+ 运行时独有状态：
/// - `signal_tx`：向 TelegramSender 发送信号
/// - `llm`：支持运行时切换的 LLM provider
/// - `mcp_tools` / `skill_manager`：启动时加载，生命周期与 handler 绑定
///
/// 供 `TelegramSender` 记录消息时读取，无需循环依赖。
///
/// 并发调度策略委托给 `session` 模块。
pub struct AgentHandler {
    pub ctx: AppContext,
    pub signal_tx: mpsc::UnboundedSender<BotSignal>,
    /// LLM 提供商（支持运行时切换）
    llm: RwLock<Arc<dyn LLMProvider>>,
    pub mcp_tools: McpTools,
    pub skill_manager: Arc<SkillManager>,
}

impl AgentHandler {
    pub async fn new(ctx: AppContext, signal_tx: mpsc::UnboundedSender<BotSignal>) -> Result<Self> {
        let config = Arc::clone(&ctx.cfg);
        let mcp_tools = Self::load_mcp_tools(&config).await?;
        let skill_manager = Arc::new(SkillManager::load(&config.skills.dirs).await?);
        let llm = Self::build_llm(&ctx)?;

        Ok(Self {
            ctx,
            signal_tx,
            llm: RwLock::new(llm),
            mcp_tools,
            skill_manager,
        })
    }

    // -------------------------------------------------------------------------
    // 事件路由
    // -------------------------------------------------------------------------

    /// 事件路由：不同 chat 并行，同一 chat 串行
    pub async fn run(
        self: Arc<Self>,
        mut event_rx: mpsc::UnboundedReceiver<AgentEvent>,
    ) -> Result<()> {
        let mut sessions: HashMap<i64, mpsc::UnboundedSender<AgentEvent>> = HashMap::new();

        while let Some(event) = event_rx.recv().await {
            let chat_id = event.chat_id();

            // 若 session 已退出则重建
            if sessions.get(&chat_id).is_none_or(|tx| tx.is_closed()) {
                sessions.insert(chat_id, spawn_chat_session(Arc::clone(&self), chat_id));
            }

            if let Err(e) = sessions[&chat_id].send(event) {
                tracing::error!(chat_id, "Failed to send event to chat session: {e}");
            }
        }
        Ok(())
    }

    // -------------------------------------------------------------------------
    // 执行
    // -------------------------------------------------------------------------

    /// 构建任务消息并运行 Agent
    pub async fn execute(&self, chat_id: i64, events: &[AgentEvent]) -> Result<()> {
        let events = preprocess_events(events);
        let causes: Vec<&str> = events.causes().map(|c| c.label()).collect();
        tracing::info!(chat_id, triggers = ?causes, "Agent woke up");

        self.notify_typing(chat_id);

        let ctx = self
            .ctx
            .agent
            .context_fty
            .build_context(
                self.ctx.bot.identity.clone(),
                chat_id,
                self.ctx.cfg.agent.context.message_history_limit,
            )
            .await?;

        let message_ids: Vec<i64> = ctx.message_ids.clone();
        let task_message = render_main_context(&ctx, build_trigger_section(events));
        tracing::info!(chat_id, "Agent task message:\n{task_message}");

        let response: String = self
            .main_agent_handle(chat_id, ctx.chat.chat_type())
            .await?
            .agent
            .run(Task::new(task_message))
            .await
            .change_err_msg(ErrorKind::Internal, "Agent execution failed")?;

        if !message_ids.is_empty() {
            if let Err(e) = self.ctx.db.srv.message.mark_unread_seen(&message_ids).await {
                tracing::warn!(chat_id, "Failed to mark messages seen: {e}");
            } else {
                tracing::debug!(n = message_ids.len(), "Marked messages seen");
            }
        }

        tracing::info!(chat_id, response, "Agent done");
        Ok(())
    }

    /// 通知平台显示"正在输入"状态
    fn notify_typing(&self, chat_id: i64) {
        let _ = self.signal_tx.send(BotSignal::Typing { chat_id });
    }

    // -------------------------------------------------------------------------
    // LLM
    // -------------------------------------------------------------------------

    /// 获取当前 LLM（用于 Agent 构建）
    pub async fn main_llm(&self) -> Arc<dyn LLMProvider> {
        self.llm.read().await.clone()
    }

    fn build_llm(ctx: &AppContext) -> Result<Arc<dyn LLMProvider>> {
        let provider = ctx.provider.get_checked(&ctx.cfg.agent.provider)?;
        let agent_config = &ctx.cfg.agent;
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

    // -------------------------------------------------------------------------
    // Agent 组装
    // -------------------------------------------------------------------------

    pub async fn main_agent_handle(
        &self,
        chat_id: i64,
        chat_type: ChatType,
    ) -> Result<DirectAgentHandle<ReActAgent<MainAgent>>> {
        let mut tools = get_main_agent_tools(ToolContext {
            ctx: self.ctx.clone(),
            chat_id,
            signal_tx: self.signal_tx.clone(),
        });
        tools.extend(self.mcp_tools.get_tools().await);
        tools.extend(load_skill_tool(Arc::clone(&self.skill_manager)));

        AgentBuilder::<_, DirectAgent>::new(ReActAgent::new(MainAgent {
            tools,
            system_prompt: self.build_system_prompt(chat_type),
        }))
        .llm(self.main_llm().await)
        .memory(Box::new(SlidingWindowMemory::new(
            self.ctx.cfg.agent.context.sliding_window_size,
        )))
        .build()
        .await
        .change_err_msg(ErrorKind::Internal, "Agent builder failed")
    }

    /// System Prompt = 人格画像 + 场景 + 工具手册 + 用户自定义 + Skills
    ///
    /// 组装顺序的设计意图：
    /// 1. 人格画像放最前面——先让 LLM 建立"我是谁"的认知
    /// 2. 场景紧跟人格——形成完整的角色认知（我是谁 + 我在什么环境）
    /// 3. 工具手册——知道自己的角色后再看能用什么工具
    /// 4. 用户自定义 / Skills——叠加层
    pub fn build_system_prompt(&self, chat_type: ChatType) -> String {
        let config = &self.ctx.cfg;
        let personality_prompt = personality_context(&self.ctx.agent.personality);
        let scene = match chat_type {
            ChatType::Private => &config.agent.context.private_prompt,
            ChatType::Group | ChatType::Supergroup => &config.agent.context.group_prompt,
            // TODO
            ChatType::Channel => "",
        };

        let mut prompt = personality_prompt;

        if !scene.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(scene);
        }

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

    // -------------------------------------------------------------------------
    // MCP
    // -------------------------------------------------------------------------

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
        .change_err_msg(ErrorKind::Internal, "Failed to load MCP tools")
    }
}

// =============================================================================
// 内部工具
// =============================================================================

fn preprocess_events(events: &[AgentEvent]) -> Vec<&AgentEvent> {
    let mut seen = HashSet::new();
    let mut items = Vec::new();

    for event in events {
        let reason = event.cause();
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

fn build_trigger_section<'a>(events: impl IntoIterator<Item = &'a AgentEvent>) -> Section {
    let mut items = Vec::new();
    for event in events.into_iter() {
        items.push(item("context").with_content(event.cause().describe()));
    }

    if items.len() == 1 {
        section("situation").add_child(items.remove(0))
    } else {
        section("situation").add_children(items)
    }
}
