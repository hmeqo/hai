mod debounce;
mod session;
mod task;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;
use autoagents::{core::agent::DirectAgentHandle, llm::LLMProvider, prelude::*};
use autoagents_toolkit::mcp::{McpConfig, McpServerConfig, McpTools};
use base64::{Engine, prelude::BASE64_STANDARD};
use sqlx::PgPool;
use tokio::sync::{RwLock, mpsc};

use crate::{
    agent::{
        MainAgent,
        components::render_main_context,
        event::{AgentEvent, AgentEvents, BotSignal},
        multimodal::{DataImage, MultimodalService},
        personality::PersonalityMgr,
        prompts::{TOOL_MANUAL, personality_context},
        render::{Section, item, section},
        skills::{SkillManager, load_skill_tool},
        tools::get_main_agent_tools,
    },
    config::{AppConfig, ProviderManager},
    domain::{entity::ChatType, service::Services, vo::ImageOptions},
    infra::platform::telegram::BotIdentity,
};

pub use session::spawn_chat_session;

/// 核心处理器
///
/// 职责：提供所有的上下文依赖(DB, 配置, 服务) 和 Agent 组装方法。
/// 并发调度委托给 session 模块。
pub struct AgentHandler {
    pub config: Arc<AppConfig>,
    pub providers: ProviderManager,
    pub pool: PgPool,
    pub mcp_tools: McpTools,
    pub skill_manager: Arc<SkillManager>,
    /// LLM 提供商（支持运行时切换模型）
    llm: RwLock<Arc<dyn LLMProvider>>,
    /// 当前模型名称（用于展示）
    model_name: RwLock<String>,
    /// 多模态服务（图像生成、音频分析）
    pub multimodal: MultimodalService,
    pub signal_tx: mpsc::UnboundedSender<BotSignal>,
    pub services: Arc<Services>,
    pub personality: PersonalityMgr,
    pub bot: BotIdentity,
}

impl AgentHandler {
    pub async fn new(
        config: &Arc<AppConfig>,
        pool: PgPool,
        providers: ProviderManager,
        multimodal: MultimodalService,
        services: Arc<Services>,
        signal_tx: mpsc::UnboundedSender<BotSignal>,
        personality: PersonalityMgr,
        bot: BotIdentity,
    ) -> Result<Self> {
        let config = Arc::clone(config);
        let mcp_tools = Self::load_mcp_tools(&config).await?;
        let skill_manager = Arc::new(SkillManager::load(&config.skills.dirs).await?);
        let model_name = config.agent.default_model.clone();
        let llm = Self::build_llm(&providers, &config.agent)?;

        Ok(Self {
            config,
            providers,
            pool,
            mcp_tools,
            skill_manager,
            llm: RwLock::new(llm),
            model_name: RwLock::new(model_name),
            multimodal,
            signal_tx,
            services,
            personality,
            bot,
        })
    }

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

    /// 构建任务消息并运行 Agent
    pub async fn execute(&self, chat_id: i64, events: &[AgentEvent]) -> Result<()> {
        let events = preprocess_events(events);
        let causes: Vec<&str> = events.causes().map(|c| c.label()).collect();
        tracing::info!(chat_id, triggers = ?causes, "Agent woke up");

        self.notify_typing(chat_id);

        let ctx = self
            .services
            .context
            .build_context(
                self.bot.clone(),
                chat_id,
                self.config.agent.context.message_history_limit,
            )
            .await?;

        let message_ids: Vec<i64> = ctx.messages.message_ids.clone();

        let task_message = render_main_context(&ctx, build_trigger_section(events));
        tracing::info!(chat_id, "Agent task message:\n{task_message}");

        let _resp: String = self
            .main_agent_handle(chat_id, ctx.chat.chat_type())
            .await?
            .agent
            .run(Task::new(task_message))
            .await
            .map_err(Into::<anyhow::Error>::into)?;

        if !message_ids.is_empty() {
            if let Err(e) = self.services.message.mark_unread_seen(&message_ids).await {
                tracing::warn!(chat_id, "Failed to mark messages seen: {e}");
            } else {
                tracing::debug!(n = message_ids.len(), "Marked messages seen");
            }
        }

        tracing::info!(chat_id, "Agent done");
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

    /// 获取当前模型名称（用于展示）
    pub async fn current_model(&self) -> String {
        self.model_name.read().await.clone()
    }

    /// 切换模型（保留当前 provider 不变）
    pub async fn switch_model(&self, model: &str) -> Result<()> {
        let mut cfg = self.config.agent.clone();
        cfg.default_model = model.to_string();
        let new_llm = Self::build_llm(&self.providers, &cfg)?;
        *self.llm.write().await = new_llm;
        *self.model_name.write().await = model.to_string();
        Ok(())
    }

    /// 根据配置构建 LLM provider
    ///
    /// 从 providers 配置池中查找当前 provider，构建 LLM。
    /// 所有 provider 统一返回 `Arc<dyn LLMProvider>`，供 AgentBuilder 消费。
    fn build_llm(
        providers: &ProviderManager,
        agent_config: &crate::config::schema::AgentConfig,
    ) -> Result<Arc<dyn LLMProvider>> {
        use crate::agent::provider::LlmBuildConfig;

        let provider = providers.get_checked(&agent_config.provider)?;
        let effort = agent_config.reasoning_effort()?;

        let build_cfg = LlmBuildConfig {
            api_key: provider.config.api_key.clone(),
            base_url: provider.base_url.clone(),
            model: agent_config.default_model.clone(),
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
        let mut tools =
            get_main_agent_tools(Arc::clone(&self.services), chat_id, self.signal_tx.clone());
        tools.extend(self.mcp_tools.get_tools().await);

        // 若有 skills 可用，注入 load_skill 元工具
        if !self.skill_manager.is_empty() {
            tools.push(load_skill_tool(Arc::clone(&self.skill_manager)));
        }

        AgentBuilder::<_, DirectAgent>::new(ReActAgent::new(MainAgent {
            tools,
            system_prompt: self.build_system_prompt(chat_type),
        }))
        .llm(self.main_llm().await)
        .memory(Box::new(SlidingWindowMemory::new(
            self.config.agent.context.sliding_window_size,
        )))
        .build()
        .await
        .map_err(Into::into)
    }

    /// System Prompt = 人格画像 + 场景 + 工具手册 + 用户自定义 + Skills
    ///
    /// 组装顺序的设计意图：
    /// 1. 人格画像放最前面——先让 LLM 建立"我是谁"的认知
    /// 2. 场景紧跟人格——形成完整的角色认知（我是谁 + 我在什么环境）
    /// 3. 工具手册——知道自己的角色后再看能用什么工具
    /// 4. 用户自定义 / Skills——叠加层
    pub fn build_system_prompt(&self, chat_type: ChatType) -> String {
        let personality_prompt = personality_context(&self.personality);
        let scene = match chat_type {
            ChatType::Private => &self.config.agent.context.private_prompt,
            ChatType::Group | ChatType::Supergroup => &self.config.agent.context.group_prompt,
            // TODO
            ChatType::Channel => "",
        };

        // 人格 + 场景 紧密组合，形成完整角色认知
        let mut prompt = personality_prompt;

        if !scene.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(scene);
        }

        // 工具手册放在角色认知之后
        prompt.push_str("\n\n");
        prompt.push_str(TOOL_MANUAL);

        if !self.config.agent.context.system_prompt.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(&self.config.agent.context.system_prompt);
        }

        // 注入 skills 发现列表（Level 1）
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
        .map_err(Into::into)
    }

    // -------------------------------------------------------------------------
    // 多模态（对外 API：bot 层无法直接访问 multimodal 字段）
    // -------------------------------------------------------------------------

    pub async fn image(&self, opts: ImageOptions) -> Result<DataImage> {
        if let Some(url) = opts.image_url {
            self.multimodal
                .image
                .generate_image_with_image_url(&opts.prompt, &url)
                .await
        } else {
            self.multimodal.image.generate_image(&opts.prompt).await
        }
    }

    pub async fn analyze_audio(&self, prompt: &str, audio: &[u8], format: &str) -> Result<String> {
        self.multimodal
            .audio
            .analyze_audio(prompt, &BASE64_STANDARD.encode(audio), format)
            .await
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
