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
        events::AgentEventBus,
        model::{ModelCatalog, ModelEntry},
        multimodal::MultimodalService,
        prompt::layers::{PromptLayers, resolve as resolve_prompt_layers},
        run::SelectedModel,
        web::WebService,
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
    pub prompts: PromptLayers,
    pub web: WebService,
}

impl AgentRuntime {
    pub async fn new(app: AppContext) -> Result<Self> {
        let cfg = Arc::clone(&app.cfg);
        let mcp_manager = McpManager::from_config(&cfg.mcp).await?;
        let skill_manager = SkillManager::load(&cfg.skills.dirs, &cfg.skills.disabled).await?;
        let prompts = resolve_prompt_layers(&cfg)?;
        let web = WebService::from_config(&cfg.web)?;

        let chat = &app.chat_transport;
        let client = provider::create_genai_client(chat.kind, &chat.target)?;

        let model = chat.target.model.clone();
        let mut clients = HashMap::new();
        for entry in app.model_catalog.chat_entries() {
            let transport = entry.chat.as_ref().expect("a chat entry has a transport");
            clients.insert(
                ModelAlias::new(entry.alias.clone()),
                provider::create_genai_client(transport.kind, &transport.target)?,
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

    pub(crate) fn model_catalog(&self) -> &ModelCatalog {
        &self.app.model_catalog
    }

    /// A stored alias the catalog no longer has resolves to the default; the caller clears the
    /// stale preference.
    pub(crate) fn model_exists(&self, alias: &ModelAlias) -> bool {
        self.app
            .model_catalog
            .get(alias.as_str())
            .is_some_and(|entry| entry.chat.is_some())
    }

    pub(crate) fn resolve_model(&self, alias: Option<&ModelAlias>) -> SelectedModel {
        if let Some(alias) = alias
            && let Some(entry) = self.app.model_catalog.get(alias.as_str())
            && entry.chat.is_some()
            && let Some(client) = self.clients.get(alias)
        {
            return selected_model(Some(alias.clone()), client.clone(), entry, self.cfg());
        }
        let default_alias = self.cfg().model_roles.default.model.as_str();
        if let Some(entry) = self.app.model_catalog.get(default_alias) {
            return selected_model(None, self.client.clone(), entry, self.cfg());
        }
        SelectedModel {
            alias: None,
            name: None,
            client: self.client.clone(),
            model: self.model.clone(),
            chat: self.cfg().model_roles.default.clone(),
            capabilities: Default::default(),
            max_tokens: None,
            reasoning_effort: None,
        }
    }
}

fn selected_model(
    alias: Option<ModelAlias>,
    client: Client,
    entry: &ModelEntry,
    cfg: &AppConfig,
) -> SelectedModel {
    let mut chat = cfg.model_roles.default.clone();
    chat.temperature = chat.temperature.or(entry.config.temperature);
    let identity = entry.identity();
    let reasoning_effort = entry.reasoning_effort(chat.thinking);
    SelectedModel {
        alias,
        name: entry.config.name.clone(),
        client,
        model: identity.model.to_owned(),
        chat,
        capabilities: entry.capabilities(),
        max_tokens: entry.config.max_tokens,
        reasoning_effort,
    }
}

#[cfg(test)]
mod tests {
    use genai::chat::ReasoningEffort;

    use super::*;
    use crate::{
        agentcore::{ModelKind, ThinkingEffort},
        config::schema::{InputModality, ModelConfig, ThinkingConfig, ThinkingLevel},
    };

    fn entry(temperature: Option<f32>) -> ModelEntry {
        ModelEntry {
            alias: "sonnet".into(),
            config: ModelConfig {
                model: "openrouter/anthropic/claude-sonnet-4.5".into(),
                name: Some("Claude Sonnet".into()),
                roles: [ModelKind::Chat].into(),
                input: [InputModality::Text, InputModality::Image].into(),
                max_tokens: Some(4096),
                context_window: None,
                temperature,
                thinking: Some(ThinkingConfig {
                    efforts: vec![ThinkingEffort::Low, ThinkingEffort::High],
                    default: Some(ThinkingEffort::High),
                }),
                pricing: None,
            },
            chat: None,
        }
    }

    #[test]
    fn a_role_temperature_overrides_the_model_default() {
        let mut cfg = AppConfig::default();
        let selected = selected_model(
            None,
            crate::agent::test_support::genai_client(),
            &entry(Some(0.6)),
            &cfg,
        );
        assert_eq!(selected.chat.temperature, Some(0.6));
        assert!(selected.capabilities.image_input);
        assert_eq!(selected.max_tokens, Some(4096));
        assert_eq!(selected.name.as_deref(), Some("Claude Sonnet"));
        assert_eq!(
            selected
                .reasoning_effort
                .as_ref()
                .map(ReasoningEffort::variant_name),
            Some("high")
        );

        cfg.model_roles.default.temperature = Some(0.2);
        cfg.model_roles.default.thinking = ThinkingLevel::Effort(ThinkingEffort::Low);
        let selected = selected_model(
            None,
            crate::agent::test_support::genai_client(),
            &entry(Some(0.6)),
            &cfg,
        );
        assert_eq!(selected.chat.temperature, Some(0.2));
        assert_eq!(
            selected
                .reasoning_effort
                .as_ref()
                .map(ReasoningEffort::variant_name),
            Some("low")
        );

        cfg.model_roles.default.thinking = ThinkingLevel::Off;
        let selected = selected_model(
            None,
            crate::agent::test_support::genai_client(),
            &entry(Some(0.6)),
            &cfg,
        );
        assert_eq!(
            selected
                .reasoning_effort
                .as_ref()
                .map(ReasoningEffort::variant_name),
            Some("zero")
        );
    }

    #[test]
    fn no_model_or_role_temperature_sends_none() {
        let cfg = AppConfig::default();
        let selected = selected_model(
            None,
            crate::agent::test_support::genai_client(),
            &entry(None),
            &cfg,
        );
        assert_eq!(selected.chat.temperature, None);
    }
}
