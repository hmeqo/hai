//! The process's own agent machinery: the container everything is assembled into (`AppContext`),
//! the handles built from it once (`AgentRuntime`), which chats have an agent (`AgentRegistry`), and
//! the timers that start with the process.

pub(crate) mod context;
pub(crate) mod registry;
pub(crate) mod startup_refeed;
pub(crate) mod task_watcher;

use std::{collections::HashMap, sync::Arc};

pub use context::AppContext;
pub(crate) use context::assemble;
use derive_more::Deref;
use genai::Client;

use crate::{
    agent::{
        events::AgentEventBus, multimodal::MultimodalService, run::SelectedModel, web::WebService,
    },
    agentcore::{mcp::McpManager, provider, skills::SkillManager},
    config::AppConfig,
    domain::{service::Services, vo::ModelAlias},
    error::Result,
};

#[derive(Clone, Deref)]
pub struct AgentRuntime(pub(crate) Arc<AgentRuntimeInner>);

pub struct AgentRuntimeInner {
    pub app: AppContext,
    pub client: Client,
    pub model: String,
    pub clients: HashMap<ModelAlias, Client>,
    pub mcp_manager: McpManager,
    pub skill_manager: SkillManager,
    pub prompts: crate::agent::prompt::layers::PromptLayers,
    pub web: WebService,
}

impl AgentRuntime {
    pub async fn new(app: AppContext) -> Result<Self> {
        let cfg = Arc::clone(&app.cfg);
        let mcp_manager = McpManager::from_config(&cfg.mcp).await?;
        let skill_manager = SkillManager::load(&cfg.skills.dirs, &cfg.skills.disabled).await?;
        let prompts = crate::agent::prompt::layers::resolve(&cfg)?;
        let web = WebService::from_config(&cfg.web)?;

        let chat = &app.chat_transport;
        let client = provider::create_genai_client(chat.kind, &chat.target)?;

        let model = chat.target.model.clone();
        let mut clients = HashMap::new();
        for choice in app.model_catalog.aliases() {
            clients.insert(
                ModelAlias::new(choice.alias.clone()),
                provider::create_genai_client(choice.transport.kind, &choice.transport.target)?,
            );
        }

        Ok(Self(Arc::new(AgentRuntimeInner {
            app,
            client,
            model,
            clients,
            mcp_manager,
            skill_manager,
            prompts,
            web,
        })))
    }

    /// The process-wide values every reader asks the runtime for.
    pub(crate) fn cfg(&self) -> &AppConfig {
        &self.app.cfg
    }

    pub(crate) fn services(&self) -> &Services {
        &self.app.services
    }

    pub(crate) fn event_bus(&self) -> &AgentEventBus {
        &self.app.event_bus
    }

    pub(crate) fn multimodal(&self) -> &MultimodalService {
        &self.app.multimodal
    }

    /// A stored alias the catalog no longer has resolves to the default; the caller clears the
    /// stale preference.
    pub(crate) fn model_exists(&self, alias: &ModelAlias) -> bool {
        self.app.model_catalog.get(alias.as_str()).is_some()
    }

    pub(crate) fn resolve_model(&self, alias: Option<&ModelAlias>) -> SelectedModel {
        if let Some(alias) = alias
            && let Some(choice) = self.app.model_catalog.get(alias.as_str())
            && let Some(client) = self.clients.get(alias)
        {
            return SelectedModel {
                alias: Some(alias.clone()),
                client: client.clone(),
                model: choice.transport.target.model.clone(),
                chat: choice.chat.clone(),
            };
        }
        SelectedModel {
            alias: None,
            client: self.client.clone(),
            model: self.model.clone(),
            chat: self.cfg().model_roles.default.clone(),
        }
    }
}
