//! The `[models]` catalog: named `provider/model` entries a Conversation may select without
//! touching `[model-roles.default]`.

use std::collections::BTreeMap;

use super::{provider::ProviderRegistry, role::split_model};
use crate::{
    agentcore::{ModelKind, provider::ChatTransport},
    config::{AppConfig, schema::ChatConfig},
    error::{AppError, Result},
};

#[derive(Debug, Clone)]
pub(crate) struct ModelChoice {
    pub alias: String,
    pub chat: ChatConfig,
    pub transport: ChatTransport,
}

#[derive(Debug, Default)]
pub(crate) struct ModelCatalog {
    choices: BTreeMap<String, ModelChoice>,
}

impl ModelCatalog {
    pub(crate) fn resolve(config: &AppConfig, registry: &ProviderRegistry) -> Result<Self> {
        let mut choices = BTreeMap::new();
        for (alias, model) in &config.models {
            if alias.is_empty() || alias.chars().any(char::is_whitespace) || alias == "reset" {
                return Err(AppError::message(format!(
                    "invalid model alias {alias:?}: use a non-empty name without whitespace; `reset` is reserved for /model --reset"
                )));
            }
            let (provider, model_id) = split_model(model)?;
            let kind = registry.provider_kind(provider)?;
            let resolved = registry.endpoint(ModelKind::Chat, provider, model_id, None)?;
            let default = &config.model_roles.default;
            choices.insert(
                alias.clone(),
                ModelChoice {
                    alias: alias.clone(),
                    chat: ChatConfig {
                        model: model.clone(),
                        max_tokens: default.max_tokens,
                        reasoning: default.reasoning,
                        reasoning_effort: default.reasoning_effort.clone(),
                        temperature: default.temperature,
                    },
                    transport: ChatTransport {
                        kind,
                        target: resolved,
                    },
                },
            );
        }
        if let Some(alias) = &config.conversation.model
            && !choices.contains_key(alias)
        {
            return Err(AppError::message(format!(
                "[conversation].model {alias:?} is not a [models] entry"
            )));
        }
        Ok(Self { choices })
    }

    pub(crate) fn get(&self, alias: &str) -> Option<&ModelChoice> {
        self.choices.get(alias)
    }

    pub(crate) fn aliases(&self) -> impl Iterator<Item = &ModelChoice> {
        self.choices.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::ProviderConfig;

    fn config() -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.model_roles.default.model = "openai/base-model".into();
        cfg.providers
            .insert("openai".into(), ProviderConfig::default());
        cfg.models.insert("fast".into(), "openai/fast-model".into());
        cfg
    }

    #[test]
    fn an_alias_names_a_provider_and_model() {
        let cfg = config();
        let registry = ProviderRegistry::from_sources(&cfg, &|_| Some("test".into()), &|_| true);
        let catalog = ModelCatalog::resolve(&cfg, &registry).unwrap();
        let choice = catalog.get("fast").unwrap();
        assert_eq!(choice.transport.target.model, "fast-model");
        assert_eq!(
            choice.transport.target.base_url,
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn a_conversation_default_must_name_a_configured_alias() {
        let mut cfg = config();
        cfg.conversation.model = Some("missing".into());
        let registry = ProviderRegistry::from_sources(&cfg, &|_| Some("test".into()), &|_| true);
        assert!(ModelCatalog::resolve(&cfg, &registry).is_err());
    }
}
