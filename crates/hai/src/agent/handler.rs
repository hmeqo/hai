use std::sync::Arc;

use anyhow::Result;
use arc_swap::ArcSwap;
use autoagents::{
    core::agent::DirectAgentHandle, llm::backends::openrouter::OpenRouter, prelude::*,
};
use autoagents_toolkit::mcp::{McpConfig, McpServerConfig, McpTools};
use base64::{Engine, prelude::BASE64_STANDARD};
use sqlx::PgPool;
use tap::Tap;
use tokio::sync::mpsc;

use crate::{
    agent::{
        multimodal::{DataImage, MultimodalService},
        node::HaiAgent,
        tools::agent::get_main_agent_tools,
    },
    app::{domain::entity::ChatType, service::ServiceContext},
    config::{AppConfig, schema::AgentConfig},
    trigger::{AgentEvent, BotSignal, TriggerReason},
};

pub type MainModel = OpenRouter;

pub struct AgentHandler {
    pub config: Arc<AppConfig>,
    pub pool: PgPool,
    pub mcp_tools: McpTools,
    pub llm: ArcSwap<MainModel>,
    pub multimodal: MultimodalService,
    pub signal_tx: mpsc::UnboundedSender<BotSignal>,
    pub services: Arc<ServiceContext>,
    pub bot_account_id: i64,
}

impl AgentHandler {
    pub async fn new(
        config: &Arc<AppConfig>,
        pool: PgPool,
        multimodal: MultimodalService,
        services: Arc<ServiceContext>,
        signal_tx: mpsc::UnboundedSender<BotSignal>,
        bot_account_id: i64,
    ) -> Result<Self> {
        let config = Arc::clone(config);
        let mcp_tools = Self::load_mcp_tools(&config).await?;
        let llm = ArcSwap::from(Self::build_llm(&config.agent, None)?);

        Ok(Self {
            config,
            pool,
            mcp_tools,
            llm,
            multimodal,
            signal_tx,
            services,
            bot_account_id,
        })
    }

    /// 不同 chat 并行，同一 chat 串行
    pub async fn run(
        self: Arc<Self>,
        mut event_rx: mpsc::UnboundedReceiver<AgentEvent>,
    ) -> Result<()> {
        let mut chat_queues: std::collections::HashMap<i64, mpsc::UnboundedSender<AgentEvent>> =
            std::collections::HashMap::new();

        while let Some(event) = event_rx.recv().await {
            let chat_id = event.chat_id();
            let tx = chat_queues
                .entry(chat_id)
                .or_insert_with(|| Self::spawn_chat_worker(Arc::clone(&self), chat_id));
            let _ = tx.send(event);
        }
        Ok(())
    }

    fn spawn_chat_worker(handler: Arc<Self>, chat_id: i64) -> mpsc::UnboundedSender<AgentEvent> {
        let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();
        let debounce = tokio::time::Duration::from_millis(handler.config.agent.debounce_ms);

        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                let mut events = vec![event];

                // 非 bypass 事件进入防抖收集
                if !events[0].reason().should_bypass_debounce() {
                    let deadline = tokio::time::Instant::now() + debounce;

                    loop {
                        let timeout = tokio::time::sleep_until(deadline);
                        tokio::pin!(timeout);

                        tokio::select! {
                            biased;
                            // 优先检查是否超时
                            _ = timeout => break,
                            // 等待新事件
                            next = rx.recv() => match next {
                                Some(ev) if ev.reason().should_bypass_debounce() => {
                                    events.push(ev); // bypass 事件中断防抖
                                    break;
                                }
                                Some(ev) => events.push(ev),
                                None => return, // channel 关闭，worker 退出
                            },
                        }
                    }
                }

                tracing::debug!("AgentHandler chat_id={chat_id} events: {events:#?}");

                if let Err(e) = handler.coordinated_prompt(chat_id, &events).await {
                    tracing::error!("AgentHandler chat_id={chat_id} error: {e}");
                }
            }
        });
        tx
    }

    pub fn main_llm(&self) -> Arc<MainModel> {
        self.llm.load_full()
    }

    pub async fn main_agent_handle(
        &self,
        chat_id: i64,
        chat_type: ChatType,
    ) -> Result<DirectAgentHandle<ReActAgent<HaiAgent>>> {
        let sliding_window_memory = Box::new(SlidingWindowMemory::new(20));
        let mcp_tools = self.mcp_tools.get_tools().await;
        AgentBuilder::<_, DirectAgent>::new(ReActAgent::new(HaiAgent {
            tools: {
                let mut tools = get_main_agent_tools(
                    Arc::clone(&self.services),
                    chat_id,
                    self.bot_account_id,
                    self.signal_tx.clone(),
                );
                tools.extend(mcp_tools);
                tools
            },
            system_prompt: self.build_system_prompt(chat_type),
        }))
        .llm(self.main_llm())
        .memory(sliding_window_memory)
        .build()
        .await
        .map_err(Into::into)
    }

    /// 构建静态 System Prompt（人格 + 场景 + 工具规范）
    ///
    /// 这部分内容在每次运行中保持不变，描述 Agent "是谁、在什么场景下、怎么使用工具"。
    /// 不包含任何本次任务的具体指令，那些由动态 Task Message 负责。
    pub fn build_system_prompt(&self, chat_type: ChatType) -> String {
        use crate::agent::prompts::*;

        // 1. 人格层（可通过 config 覆盖整个人格）
        let mut prompt = BUILTIN_PROMPT.to_owned();

        // 3. 工具使用规范层（固定，一直需要）
        prompt.push_str("\n");
        prompt.push_str(&self.config.agent.system_prompt);
        prompt.push_str("\n");

        // 2. 场景层（根据聊天类型选择场景描述）
        let scene_prompt = match chat_type {
            ChatType::Private => &self.config.agent.private_prompt,
            ChatType::Group | ChatType::Supergroup => &self.config.agent.group_prompt,
            ChatType::Channel => {
                // TODO: Channel 专属场景
                ""
            }
        };

        if !scene_prompt.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(scene_prompt);
        }

        prompt
    }

    pub fn current_model(&self) -> String {
        self.llm.load().model.clone()
    }

    pub fn switch_model(&self, model: &str) -> Result<()> {
        self.llm
            .store(Self::build_llm(&self.config.agent, Some(model))?);
        Ok(())
    }

    /// 构建动态 Task Message 并运行 Agent
    /// Agent 根据 System Prompt 中的人格/场景规则，结合本次触发信息，自行决定行为。
    pub async fn coordinated_prompt(&self, chat_id: i64, events: &[AgentEvent]) -> Result<()> {
        let context_limit = 10;
        let ctx = self
            .services
            .context
            .build_hai_context(self.bot_account_id, chat_id, context_limit)
            .await?;

        let chat_type = ctx.common.chat.chat_type();

        // 构建触发信息段（仅描述"发生了什么"，不指定行为）
        let trigger_section = build_trigger_section(events);
        let task_message = ctx.render_as_prompt(&trigger_section);

        tracing::debug!("Agent task message:\n{}", task_message);

        self.main_agent_handle(chat_id, chat_type)
            .await?
            .agent
            .run(Task::new(task_message))
            .await
            .map_err(Into::<anyhow::Error>::into)?
            .tap(|resp| tracing::debug!("Agent Response: {resp}"));

        Ok(())
    }

    pub async fn image(&self, opts: ImageOptions) -> Result<DataImage> {
        if let Some(image_url) = opts.image_url {
            self.multimodal
                .image
                .generate_image_with_image_url(&opts.prompt, &image_url)
                .await
        } else {
            self.multimodal.image.generate_image(&opts.prompt).await
        }
    }

    pub async fn analyze_audio(
        &self,
        prompt: &str,
        input_audio: &Vec<u8>,
        format: &str,
    ) -> Result<String> {
        let input_audio = BASE64_STANDARD.encode(input_audio);
        self.multimodal
            .audio
            .analyze_audio(prompt, &input_audio, format.as_ref())
            .await
    }

    fn build_llm(config: &AgentConfig, model: Option<&str>) -> Result<Arc<MainModel>> {
        LLMBuilder::<MainModel>::new()
            .api_key(&config.api_key)
            .model(model.unwrap_or(&config.default_model))
            .reasoning(config.reasoning)
            .reasoning_effort(config.reasoning_effort()?)
            .temperature(config.temperature)
            .max_tokens(config.max_tokens)
            .build()
            .map_err(Into::into)
    }

    async fn load_mcp_tools(config: &AppConfig) -> Result<McpTools> {
        McpTools::from_config_object(&McpConfig {
            servers: config
                .mcp
                .iter()
                .map(|(name, mcp)| {
                    let mut cfg =
                        McpServerConfig::new(name.clone(), mcp.r#type.clone(), mcp.command.clone())
                            .with_args(mcp.args.clone());
                    if let Some(env) = mcp.env.as_ref() {
                        cfg = cfg.with_env(env.clone());
                    }
                    cfg
                })
                .collect(),
        })
        .await
        .map_err(Into::into)
    }
}

/// 构建触发信息段
///
/// 将本次触发的所有事件汇总成一段简洁的说明文字，
/// 仅描述"发生了什么"，不包含任何行为指令。
fn build_trigger_section(events: &[AgentEvent]) -> String {
    use std::fmt::Write;

    // 去重逻辑：
    // - 对于无附加数据的 reason（Private / Mention / Random / Command），
    //   相同 label 只保留一条（省去重复说明）
    // - 对于携带 TaskPayload 的 Cron，每个不同的 task_id + description 都保留，
    //   因为它们代表不同的任务，不能合并
    let mut seen_simple: std::collections::HashSet<&'static str> = std::collections::HashSet::new();
    let mut descriptions: Vec<String> = Vec::new();

    for event in events {
        let reason = event.reason();
        match reason {
            // 带 payload 的 Cron：每条都保留（不同任务不合并，避免信息丢失）
            TriggerReason::Cron(_) => {
                descriptions.push(reason.describe());
            }
            // 无 payload 的简单 reason（Private / Mention / Random / Active / Command）：
            // 相同类型只保留一次
            _ => {
                let label = reason.label();
                if seen_simple.insert(label) {
                    descriptions.push(reason.describe());
                }
            }
        }
    }

    let mut s = String::new();
    let _ = writeln!(s, "## Trigger Scenario");

    if descriptions.len() == 1 {
        let _ = writeln!(s, "{}", descriptions[0]);
    } else {
        for (i, desc) in descriptions.iter().enumerate() {
            let _ = writeln!(s, "{}. {}", i + 1, desc);
        }
    }

    s
}

pub struct ImageOptions {
    pub prompt: String,
    pub image_url: Option<String>,
}
