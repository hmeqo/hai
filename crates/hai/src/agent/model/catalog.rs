//! The `[models]` registry: every alias names one provider model and what that model can do.

use std::collections::{BTreeMap, BTreeSet};

use genai::chat::ReasoningEffort;

use super::{ModelCapabilities, ModelRef, provider::ProviderRegistry, split_model};
use crate::{
    agentcore::{ModelKind, provider::ChatTransport},
    config::{
        AppConfig, ConfigIssue,
        schema::{InputModality, ModelConfig, ThinkingConfig, ThinkingLevel},
    },
    error::{AppError, Result},
};

#[derive(Debug, Clone)]
pub(crate) struct ModelEntry {
    pub alias: String,
    pub config: ModelConfig,
    pub chat: Option<ChatTransport>,
}

#[derive(Debug, Default)]
pub(crate) struct ModelCatalog {
    entries: BTreeMap<String, ModelEntry>,
}

impl ModelCatalog {
    /// Every entry the config declares or a role selector spells directly.
    pub(crate) fn resolve(config: &AppConfig, registry: &ProviderRegistry) -> Result<Self> {
        Self::resolve_where(config, registry, |_| true)
    }

    /// The entries that fill one of `kinds`: a management command that needs one role does not
    /// validate the rest of the table.
    pub(crate) fn resolve_for(
        config: &AppConfig,
        registry: &ProviderRegistry,
        kinds: &[ModelKind],
    ) -> Result<Self> {
        Self::resolve_where(config, registry, |kind| kinds.contains(&kind))
    }

    fn resolve_where(
        config: &AppConfig,
        registry: &ProviderRegistry,
        wanted: impl Fn(ModelKind) -> bool,
    ) -> Result<Self> {
        let mut entries = BTreeMap::new();
        for (alias, model) in &config.models {
            if !model.roles.iter().copied().any(&wanted) {
                continue;
            }
            entries.insert(alias.clone(), entry(config, alias, model, registry)?);
        }
        for (selector, kinds) in direct_selectors(config) {
            if entries.contains_key(selector) {
                continue;
            }
            let kinds: Vec<ModelKind> = kinds.into_iter().filter(|kind| wanted(*kind)).collect();
            if kinds.is_empty() {
                continue;
            }
            entries.insert(
                selector.to_owned(),
                transient_entry(selector, &kinds, registry, config)?,
            );
        }
        Ok(Self { entries })
    }

    pub(crate) fn get(&self, alias: &str) -> Option<&ModelEntry> {
        self.entries.get(alias)
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = &ModelEntry> {
        self.entries.values()
    }

    pub(crate) fn chat_entries(&self) -> impl Iterator<Item = &ModelEntry> {
        self.entries.values().filter(|entry| entry.chat.is_some())
    }

    pub(crate) fn any_chat_image_input(&self) -> bool {
        self.chat_entries()
            .any(|entry| entry.config.input.contains(&InputModality::Image))
    }
}

impl ModelEntry {
    pub(crate) fn identity(&self) -> ModelRef<'_> {
        split_model(&self.config.model).expect("the catalog validated the model id")
    }

    pub(crate) fn supports(&self, kind: ModelKind) -> bool {
        self.config.roles.contains(&kind)
    }

    pub(crate) fn capabilities(&self) -> ModelCapabilities {
        ModelCapabilities {
            image_input: self.config.input.contains(&InputModality::Image),
        }
    }

    pub(crate) fn reasoning_effort(&self, level: ThinkingLevel) -> Option<ReasoningEffort> {
        match level {
            ThinkingLevel::Inherit => Some(self.config.thinking.as_ref()?.default?.into()),
            ThinkingLevel::Off => self.config.thinking.as_ref().map(|_| ReasoningEffort::Zero),
            ThinkingLevel::Effort(effort) => {
                self.config.thinking.as_ref()?;
                Some(effort.into())
            }
        }
    }
}

fn direct_selectors(config: &AppConfig) -> BTreeMap<&str, Vec<ModelKind>> {
    let mut selectors: BTreeMap<&str, Vec<ModelKind>> = BTreeMap::new();
    for (selector, kind) in super::role::direct_selectors(config) {
        selectors.entry(selector).or_default().push(kind);
    }
    selectors
}

fn entry(
    config: &AppConfig,
    alias: &str,
    model: &ModelConfig,
    registry: &ProviderRegistry,
) -> Result<ModelEntry> {
    validate_alias(alias)?;
    let identity = validate_model(alias, model)?;
    if model.roles.contains(&ModelKind::Chat) {
        validate_role_thinking(alias, config.model_roles.default.thinking, model)?;
    }
    registry.get_checked(identity.provider)?;
    let chat = if model.roles.contains(&ModelKind::Chat) {
        let kind = registry.provider_kind(identity.provider)?;
        let target = registry.endpoint(ModelKind::Chat, identity.provider, identity.model, None)?;
        Some(ChatTransport { kind, target })
    } else {
        None
    };
    Ok(ModelEntry {
        alias: alias.to_owned(),
        config: model.clone(),
        chat,
    })
}

fn transient_entry(
    selector: &str,
    kinds: &[ModelKind],
    registry: &ProviderRegistry,
    config: &AppConfig,
) -> Result<ModelEntry> {
    let identity = model_identity(selector, selector)?;
    registry.get_checked(identity.provider)?;
    let model = ModelConfig {
        model: selector.to_owned(),
        name: None,
        roles: kinds.iter().copied().collect(),
        input: BTreeSet::from([InputModality::Text]),
        max_tokens: None,
        context_window: None,
        temperature: None,
        thinking: None,
        pricing: None,
    };
    validate_capabilities(selector, &model)?;
    if kinds.contains(&ModelKind::Chat) {
        validate_role_thinking(selector, config.model_roles.default.thinking, &model)?;
    }
    let chat = if kinds.contains(&ModelKind::Chat) {
        let kind = registry.provider_kind(identity.provider)?;
        let target = registry.endpoint(ModelKind::Chat, identity.provider, identity.model, None)?;
        Some(ChatTransport { kind, target })
    } else {
        None
    };
    Ok(ModelEntry {
        alias: selector.to_owned(),
        config: model,
        chat,
    })
}

fn validate_alias(alias: &str) -> Result<()> {
    if alias.is_empty() || alias.chars().any(char::is_whitespace) {
        return Err(AppError::message(format!(
            "invalid model alias {alias:?}: use a non-empty name without whitespace"
        )));
    }
    Ok(())
}

fn validate_model<'a>(alias: &str, model: &'a ModelConfig) -> Result<ModelRef<'a>> {
    let identity = model_identity(alias, &model.model)?;
    validate_capabilities(alias, model)?;
    validate_limits(alias, model)?;
    validate_sampling(alias, model)?;
    validate_pricing(alias, model)?;
    Ok(identity)
}

fn model_identity<'a>(alias: &str, raw: &'a str) -> Result<ModelRef<'a>> {
    split_model(raw).map_err(|_| {
        ConfigIssue::model(
            alias,
            "model must be provider/model with a non-empty provider and model id",
        )
        .into()
    })
}

fn validate_capabilities(alias: &str, model: &ModelConfig) -> Result<()> {
    if model.roles.is_empty() {
        return Err(ConfigIssue::model(alias, "roles must name at least one ModelRole").into());
    }
    if model.input.is_empty() {
        return Err(ConfigIssue::model(alias, "input must name at least one modality").into());
    }
    Ok(())
}

fn validate_limits(alias: &str, model: &ModelConfig) -> Result<()> {
    if model.max_tokens == Some(0) || model.context_window == Some(0) {
        return Err(ConfigIssue::model(alias, "token limits must be positive").into());
    }
    if let (Some(max_tokens), Some(context_window)) = (model.max_tokens, model.context_window)
        && max_tokens > context_window
    {
        return Err(ConfigIssue::model(alias, "max-tokens exceeds context-window").into());
    }
    Ok(())
}

fn validate_sampling(alias: &str, model: &ModelConfig) -> Result<()> {
    if model
        .temperature
        .is_some_and(|value| !value.is_finite() || value < 0.0)
    {
        return Err(
            ConfigIssue::model(alias, "temperature must be finite and non-negative").into(),
        );
    }
    if let Some(thinking) = &model.thinking {
        validate_thinking(alias, thinking)?;
    }
    Ok(())
}

fn validate_role_thinking(alias: &str, level: ThinkingLevel, model: &ModelConfig) -> Result<()> {
    match level {
        ThinkingLevel::Inherit => Ok(()),
        ThinkingLevel::Off => {
            if model.thinking.is_some() {
                Ok(())
            } else {
                Err(ConfigIssue::model(
                    alias,
                    "role thinking requires the model to declare thinking",
                )
                .into())
            }
        }
        ThinkingLevel::Effort(effort) => {
            let Some(thinking) = &model.thinking else {
                return Err(ConfigIssue::model(
                    alias,
                    "role thinking requires the model to declare thinking",
                )
                .into());
            };
            if thinking.efforts.contains(&effort) {
                Ok(())
            } else {
                Err(ConfigIssue::model(
                    alias,
                    format!(
                        "role thinking '{}' is not one of thinking.efforts",
                        effort.keyword()
                    ),
                )
                .into())
            }
        }
    }
}

fn validate_pricing(alias: &str, model: &ModelConfig) -> Result<()> {
    if let Some(pricing) = &model.pricing {
        for value in [pricing.input, pricing.cached_input, pricing.output]
            .into_iter()
            .flatten()
        {
            if !value.is_finite() || value < 0.0 {
                return Err(ConfigIssue::model(
                    alias,
                    "pricing values must be finite and non-negative",
                )
                .into());
            }
        }
    }
    Ok(())
}

fn validate_thinking(alias: &str, thinking: &ThinkingConfig) -> Result<()> {
    if thinking.efforts.is_empty() || has_duplicates(&thinking.efforts) {
        return Err(
            ConfigIssue::model(alias, "thinking.efforts must be non-empty and unique").into(),
        );
    }
    if let Some(default) = &thinking.default
        && !thinking.efforts.contains(default)
    {
        return Err(
            ConfigIssue::model(alias, "thinking.default is not one of thinking.efforts").into(),
        );
    }
    Ok(())
}

fn has_duplicates<T: PartialEq>(values: &[T]) -> bool {
    values
        .iter()
        .enumerate()
        .any(|(index, value)| values[..index].contains(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::test_support::model_config,
        agentcore::ThinkingEffort,
        config::schema::{
            InputModality, ModelPricing, ProviderConfig, SpeechConfig, ThinkingConfig,
            ThinkingLevel,
        },
    };

    fn config() -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.model_roles.default.model = "fast".into();
        cfg.providers
            .insert("openai".into(), ProviderConfig::default());
        cfg.models.insert(
            "fast".into(),
            model_config("openai", "fast-model", [ModelKind::Chat]),
        );
        cfg
    }

    fn registry(config: &AppConfig) -> ProviderRegistry {
        ProviderRegistry::from_sources(config, &|_| Some("test".into()), &|_| true)
    }

    fn catalog(config: &AppConfig) -> Result<ModelCatalog> {
        ModelCatalog::resolve(config, &registry(config))
    }

    #[test]
    fn an_alias_names_a_provider_and_model() {
        let cfg = config();
        let catalog = catalog(&cfg).unwrap();
        let entry = catalog.get("fast").unwrap();
        assert_eq!(entry.config.model, "openai/fast-model");
        assert_eq!(entry.chat.as_ref().unwrap().target.model, "fast-model");
        assert_eq!(
            entry.chat.as_ref().unwrap().target.base_url,
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn a_direct_chat_selector_needs_no_preset() {
        let mut cfg = config();
        cfg.models.clear();
        cfg.model_roles.default.model = "openai/direct-model".into();

        let catalog = catalog(&cfg).unwrap();
        let entry = catalog.get("openai/direct-model").unwrap();
        assert!(entry.chat.is_some());
        assert_eq!(entry.chat.as_ref().unwrap().target.model, "direct-model");
        assert!(!entry.capabilities().image_input);
    }

    #[test]
    fn a_direct_optional_role_selector_needs_no_preset() {
        let mut cfg = config();
        cfg.model_roles.speech = Some(SpeechConfig {
            model: Some("openai/direct-tts".into()),
            voice: "Zephyr".into(),
            speed: 1.0,
        });

        let catalog = catalog(&cfg).unwrap();
        let entry = catalog.get("openai/direct-tts").unwrap();
        assert!(entry.chat.is_none());
        assert_eq!(entry.config.roles, [ModelKind::Speech].into());
    }

    #[test]
    fn a_direct_chat_selector_rejects_effort_control_without_a_preset() {
        let mut cfg = config();
        cfg.models.clear();
        cfg.model_roles.default.model = "openai/direct-model".into();
        cfg.model_roles.default.thinking = ThinkingLevel::Effort(ThinkingEffort::High);

        assert!(catalog(&cfg).is_err());
    }

    #[test]
    fn capabilities_come_from_input_and_default_to_text_only() {
        assert!(
            !catalog(&config())
                .unwrap()
                .get("fast")
                .unwrap()
                .capabilities()
                .image_input
        );

        let mut cfg = config();
        cfg.models.get_mut("fast").unwrap().input =
            [InputModality::Text, InputModality::Image].into();
        assert!(
            catalog(&cfg)
                .unwrap()
                .get("fast")
                .unwrap()
                .capabilities()
                .image_input
        );
        assert!(catalog(&cfg).unwrap().any_chat_image_input());
    }

    #[test]
    fn a_model_is_usable_only_for_the_roles_it_declares() {
        let mut cfg = config();
        cfg.models.get_mut("fast").unwrap().roles = [ModelKind::Chat, ModelKind::Vision].into();
        let entry = catalog(&cfg).unwrap();
        let entry = entry.get("fast").unwrap();
        assert!(entry.supports(ModelKind::Chat));
        assert!(entry.supports(ModelKind::Vision));
        assert!(!entry.supports(ModelKind::Embedding));
    }

    #[test]
    fn chat_aliases_exclude_embedding_only_models() {
        let mut cfg = config();
        let mut embed = model_config("openai", "embed-model", [ModelKind::Chat]);
        embed.roles = [ModelKind::Embedding].into();
        cfg.models.insert("embed".into(), embed);
        let catalog = catalog(&cfg).unwrap();
        let aliases: Vec<&str> = catalog
            .chat_entries()
            .map(|entry| entry.alias.as_str())
            .collect();
        assert_eq!(aliases, vec!["fast"]);
    }

    #[test]
    fn thinking_validates_its_efforts() {
        let mut cfg = config();
        let mut entry = model_config("openai", "reasoning-model", [ModelKind::Chat]);
        entry.thinking = Some(ThinkingConfig {
            efforts: vec![ThinkingEffort::Low, ThinkingEffort::High],
            default: Some(ThinkingEffort::High),
        });
        cfg.models.insert("reason".into(), entry);
        assert!(catalog(&cfg).is_ok());

        let mut invalid = cfg.clone();
        invalid.models.get_mut("reason").unwrap().thinking = Some(ThinkingConfig {
            efforts: vec![ThinkingEffort::Low],
            default: Some(ThinkingEffort::High),
        });
        assert!(catalog(&invalid).is_err());

        let mut duplicate = cfg.clone();
        duplicate.models.get_mut("reason").unwrap().thinking = Some(ThinkingConfig {
            efforts: vec![ThinkingEffort::Low, ThinkingEffort::Low],
            default: Some(ThinkingEffort::Low),
        });
        assert!(catalog(&duplicate).is_err());
    }

    #[test]
    fn role_thinking_must_be_declared_by_chat_models() {
        let mut cfg = config();
        cfg.model_roles.default.thinking = ThinkingLevel::Effort(ThinkingEffort::High);
        assert!(catalog(&cfg).is_err());

        cfg.models.get_mut("fast").unwrap().thinking = Some(ThinkingConfig {
            efforts: vec![ThinkingEffort::Low, ThinkingEffort::High],
            default: Some(ThinkingEffort::High),
        });
        assert!(catalog(&cfg).is_ok());

        cfg.model_roles.default.thinking = ThinkingLevel::Effort(ThinkingEffort::Low);
        assert!(catalog(&cfg).is_ok());

        cfg.model_roles.default.thinking = ThinkingLevel::Effort(ThinkingEffort::Max);
        assert!(catalog(&cfg).is_err());

        cfg.model_roles.default.thinking = ThinkingLevel::Off;
        assert!(catalog(&cfg).is_ok());
    }

    /// A command that needs one role does not validate unrelated entries: the embedding rebuild
    /// must run even when a chat preset is broken.
    #[test]
    fn a_narrowed_catalog_ignores_unrelated_entries() {
        let mut cfg = config();
        cfg.models.insert(
            "embed".into(),
            model_config("openai", "embed-model", [ModelKind::Embedding]),
        );
        let mut broken = model_config("openai", "broken-model", [ModelKind::Chat]);
        broken.model = "missing/provider-model".into();
        cfg.models.insert("broken".into(), broken);

        assert!(catalog(&cfg).is_err());
        let narrowed =
            ModelCatalog::resolve_for(&cfg, &registry(&cfg), &[ModelKind::Embedding]).unwrap();
        assert!(narrowed.get("embed").is_some());
        assert!(narrowed.get("broken").is_none());
        assert!(narrowed.chat_entries().next().is_none());
    }

    #[test]
    fn token_and_pricing_limits_are_validated() {
        let mut cfg = config();
        cfg.models.get_mut("fast").unwrap().max_tokens = Some(0);
        assert!(catalog(&cfg).is_err());

        let mut cfg = config();
        cfg.models.get_mut("fast").unwrap().max_tokens = Some(10);
        cfg.models.get_mut("fast").unwrap().context_window = Some(5);
        assert!(catalog(&cfg).is_err());

        let mut cfg = config();
        cfg.models.get_mut("fast").unwrap().pricing = Some(ModelPricing {
            input: Some(-1.0),
            ..ModelPricing::default()
        });
        assert!(catalog(&cfg).is_err());
    }
}
