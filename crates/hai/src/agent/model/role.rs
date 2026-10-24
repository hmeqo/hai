//! The `[model-roles]` table: every role resolves through one policy, so a role is a row here and
//! nowhere else.

use strum::IntoEnumIterator;

use super::provider::ProviderRegistry;
use crate::{
    agentcore::{ModelKind, ModelTarget, provider::ChatTransport},
    config::{AppConfig, ConfigIssue},
    error::{Result, bail},
};

/// One role's model, or why it is absent.
#[derive(Debug, Clone, Copy)]
struct Row<'a> {
    policy: RolePolicy,
    model: Option<&'a str>,
}

/// What an absent or model-less block means for one role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RolePolicy {
    /// Run on the main model when that provider serves the kind; otherwise the role is off.
    InheritMain,
    /// The role is off.
    Off,
    /// The role is required: startup fails.
    Required,
}

fn row(config: &AppConfig, kind: ModelKind) -> Row<'_> {
    let roles = &config.model_roles;
    match kind {
        ModelKind::Chat => Row {
            policy: RolePolicy::Required,
            model: Some(&roles.default.model),
        },
        ModelKind::Vision => Row {
            policy: RolePolicy::InheritMain,
            model: roles.vision.as_ref().and_then(|b| b.model.as_deref()),
        },
        ModelKind::Transcription => Row {
            policy: RolePolicy::InheritMain,
            model: roles
                .transcription
                .as_ref()
                .and_then(|b| b.model.as_deref()),
        },
        ModelKind::Speech => Row {
            policy: RolePolicy::Off,
            model: roles.speech.as_ref().and_then(|b| b.model.as_deref()),
        },
        ModelKind::Image => Row {
            policy: RolePolicy::Off,
            model: roles.image.as_ref().and_then(|b| b.model.as_deref()),
        },
        ModelKind::Embedding => Row {
            policy: RolePolicy::Required,
            model: Some(&roles.embedding.model),
        },
        ModelKind::Rerank => Row {
            policy: RolePolicy::Off,
            model: roles.rerank.as_ref().and_then(|b| b.model.as_deref()),
        },
    }
}

/// The ModelRoles a process resolved: one model per role, plus the chat transport the genai client
/// is built from.
#[derive(Debug)]
pub(crate) struct ModelRoles {
    pub chat: ChatTransport,
    pub vision: Option<ModelTarget>,
    pub transcription: Option<ModelTarget>,
    pub speech: Option<ModelTarget>,
    pub image: Option<ModelTarget>,
    pub embedding: ModelTarget,
    pub rerank: Option<ModelTarget>,
}

/// Splits `provider/model` at the first slash. The provider owns the endpoint; the remainder is the
/// provider's model id.
pub(crate) fn split_model(model: &str) -> Result<(&str, &str)> {
    let (provider, model_id) = model.split_once('/').ok_or_else(|| {
        crate::error::AppError::message(format!("model {model:?} must be provider/model"))
    })?;
    if provider.is_empty() || model_id.is_empty() {
        return Err(crate::error::AppError::message(format!(
            "model {model:?} must name both provider and model"
        )));
    }
    Ok((provider, model_id))
}

/// Resolves the whole table against the providers this process can reach.
pub(crate) fn resolve(config: &AppConfig, registry: &ProviderRegistry) -> Result<ModelRoles> {
    let chat = required_role(config, registry, ModelKind::Chat)?;
    let (provider, _) = split_model(&config.model_roles.default.model)?;
    let chat = ChatTransport {
        kind: registry.provider_kind(provider)?,
        target: chat,
    };
    Ok(ModelRoles {
        chat,
        vision: resolve_role(config, registry, ModelKind::Vision)?,
        transcription: resolve_role(config, registry, ModelKind::Transcription)?,
        speech: resolve_role(config, registry, ModelKind::Speech)?,
        image: resolve_role(config, registry, ModelKind::Image)?,
        embedding: required_role(config, registry, ModelKind::Embedding)?,
        rerank: resolve_role(config, registry, ModelKind::Rerank)?,
    })
}

/// The embedding role, resolved without the rest of the table.
pub(crate) fn embedding(config: &AppConfig, registry: &ProviderRegistry) -> Result<ModelTarget> {
    required_role(config, registry, ModelKind::Embedding)
}

/// A role the process cannot do without: the table marks only chat and embedding required, and a
/// required row fails on an empty model rather than resolving to nothing.
fn required_role(
    config: &AppConfig,
    registry: &ProviderRegistry,
    kind: ModelKind,
) -> Result<ModelTarget> {
    Ok(resolve_role(config, registry, kind)?.expect("a required role always resolves to a model"))
}

/// The providers the roles name, which availability is resolved for; a block nothing declares names
/// none.
pub(crate) fn providers_named(config: &AppConfig) -> Vec<&str> {
    let mut names: Vec<&str> = ModelKind::iter()
        .filter_map(|kind| row(config, kind).model)
        .filter_map(|model| split_model(model).ok().map(|(provider, _)| provider))
        .collect();
    names.extend(
        config
            .models
            .values()
            .filter_map(|model| split_model(model).ok().map(|(provider, _)| provider)),
    );
    names
}

/// Resolves one row: the model its block names, or `None` for a role that is off.
fn resolve_role(
    config: &AppConfig,
    registry: &ProviderRegistry,
    kind: ModelKind,
) -> Result<Option<ModelTarget>> {
    let row = row(config, kind);
    if let Some(model) = row.model.filter(|model| !model.trim().is_empty()) {
        let (provider, model_id) = split_model(model)?;
        return registry.endpoint(kind, provider, model_id, None).map(Some);
    }

    match row.policy {
        RolePolicy::InheritMain => {
            let (provider, model) = split_model(&config.model_roles.default.model)?;
            if !registry.serves(kind, provider) {
                return Ok(None);
            }
            registry.endpoint(kind, provider, model, None).map(Some)
        }
        RolePolicy::Off => Ok(None),
        RolePolicy::Required => bail!(ConfigIssue::model_role(kind, model_required(kind))),
    }
}

/// The message a required role's model-less block earns; the two required roles differ in why.
fn model_required(kind: ModelKind) -> &'static str {
    match kind {
        ModelKind::Embedding => "is required (memory/retrieval depends on it) but model is empty",
        _ => "model is required (provider/model)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::schema::{EmbeddingConfig, ImageConfig, ProviderConfig, SpeechConfig};

    /// The roles these tests exercise are about resolution, not availability, so every provider is
    /// given a credential and every endpoint answers.
    fn registry(config: &AppConfig) -> ProviderRegistry {
        ProviderRegistry::from_sources(config, &|_| Some("test-key".to_owned()), &|_| true)
    }

    /// The config every case starts from: one chat role, its provider declared, plus the embedding
    /// role every process insists on.
    fn config(provider: &str, model: &str) -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.model_roles.default.model = format!("{provider}/{model}");
        cfg.providers.insert(provider.into(), Default::default());
        cfg.model_roles.embedding = EmbeddingConfig {
            model: "openai/text-embedding-3-small".into(),
            dimension: None,
        };
        cfg.providers.insert("openai".into(), Default::default());
        cfg
    }

    fn roles(cfg: &AppConfig) -> Result<ModelRoles> {
        super::resolve(cfg, &registry(cfg))
    }

    fn image(model: Option<&str>, provider: Option<&str>) -> ImageConfig {
        ImageConfig {
            model: match (model, provider) {
                (Some(model), Some(provider)) => Some(format!("{provider}/{model}")),
                (Some(model), None) => Some(format!("openrouter/{model}")),
                (None, _) => None,
            },
        }
    }

    /// An absent inheriting block runs on the main model, its endpoint included, so a proxied
    /// `[model-roles.default]` does not send the delegated call to the provider's own root.
    #[test]
    fn an_absent_inheriting_block_takes_the_main_endpoint() {
        let mut cfg = config("openai", "gpt-4o");
        cfg.providers.insert(
            "openai".into(),
            ProviderConfig {
                base_url: Some("https://gateway.internal/v1".into()),
                ..Default::default()
            },
        );

        let roles = roles(&cfg).unwrap();
        let vision = roles.vision.unwrap();
        assert_eq!(vision.base_url, "https://gateway.internal/v1");
        assert_eq!(vision.model, "gpt-4o");
        assert_eq!(
            roles.transcription.unwrap().base_url,
            "https://gateway.internal/v1"
        );
    }

    /// An absent inheriting block stays off when the main provider cannot read that media, instead
    /// of failing startup over a block nobody configured.
    #[test]
    fn an_absent_inheriting_block_is_off_when_the_main_provider_cannot_read_the_media() {
        let cfg = config("anthropic", "claude-sonnet-4-5");

        let roles = roles(&cfg).unwrap();
        assert!(roles.vision.is_none());
        assert!(roles.transcription.is_none());
        assert!(roles.image.is_none());
        assert!(roles.speech.is_none());
        assert!(roles.rerank.is_none());
    }

    /// A block that names a model and no provider runs on the `default` block's provider, which is
    /// where its endpoint comes from too.
    #[test]
    fn a_named_model_runs_on_the_default_provider() {
        let mut cfg = config("openrouter", "openai/gpt-4o");
        cfg.providers.insert(
            "openrouter".into(),
            ProviderConfig {
                base_url: Some("https://gateway.internal/v1".into()),
                ..Default::default()
            },
        );
        cfg.model_roles.image = Some(image(Some("some-image-model"), None));

        let roles = roles(&cfg).unwrap();
        assert_eq!(roles.image.unwrap().base_url, "https://gateway.internal/v1");
    }

    /// A dedicated block that names a provider and no model is off, and a speech block in that state
    /// is off before its voice is even looked at.
    #[test]
    fn a_dedicated_block_with_no_model_is_off() {
        let mut cfg = config("openai", "gpt-4o");
        cfg.providers
            .insert("openrouter".into(), Default::default());
        cfg.model_roles.image = Some(image(None, Some("openrouter")));
        cfg.model_roles.speech = Some(SpeechConfig {
            model: None,
            voice: String::new(),
            speed: 1.0,
        });
        cfg.model_roles.rerank = Some(crate::config::schema::RerankConfig { model: None });

        let roles = roles(&cfg).unwrap();
        assert!(roles.image.is_none());
        assert!(roles.speech.is_none());
        assert!(roles.rerank.is_none());
    }

    /// A rerank block that names only a model runs on the `default` provider, which is where an
    /// OpenRouter cross-encoder lives.
    #[test]
    fn a_rerank_block_takes_the_default_provider_and_the_kinds_own_endpoint() {
        let mut cfg = config("openrouter", "openai/gpt-4o");
        cfg.model_roles.rerank = Some(crate::config::schema::RerankConfig {
            model: Some("openrouter/cohere/rerank-4-fast".into()),
        });

        let roles = roles(&cfg).unwrap();
        assert_eq!(
            roles.rerank.unwrap().base_url,
            "https://openrouter.ai/api/v1/rerank"
        );
    }

    /// A required role with no model fails startup, with the reason embedding is required.
    #[test]
    fn a_required_role_with_no_model_fails() {
        let mut cfg = config("openai", "gpt-4o");
        cfg.model_roles.embedding.model = String::new();

        let err = roles(&cfg).unwrap_err();
        assert!(
            err.to_string()
                .contains("[model-roles.embedding] is required"),
            "{err}"
        );
    }

    /// An empty `default` provider is the one key the message points at, whichever role inherited it.
    #[test]
    fn an_unnamed_default_provider_points_at_the_default_block() {
        let mut cfg = config("openai", "gpt-4o");
        cfg.model_roles.default.model = String::new();
        cfg.model_roles.image = Some(image(Some("some-image-model"), None));

        let err = roles(&cfg).unwrap_err();
        assert!(err.to_string().contains("model is required"), "{err}");
    }

    /// The chat role is required like embedding is, for the one thing a Step loop cannot do without.
    #[test]
    fn chat_without_a_model_fails() {
        let mut cfg = config("openai", "gpt-4o");
        cfg.model_roles.default.model = String::new();

        let err = roles(&cfg).unwrap_err();
        assert!(err.to_string().contains("model is required"), "{err}");
    }

    /// Availability is resolved for the providers the roles name; a role nobody declared, and an
    /// unnamed `default` provider, name none.
    #[test]
    fn providers_named_lists_the_default_and_every_named_role() {
        let mut cfg = config("openai", "gpt-4o");
        cfg.model_roles.image = Some(image(Some("some-image-model"), Some("openrouter")));
        assert!(providers_named(&cfg).contains(&"openai"));
        assert!(providers_named(&cfg).contains(&"openrouter"));

        cfg.model_roles.default.model = String::new();
        assert!(!providers_named(&cfg).contains(&""));
    }

    /// A declared block on a provider that cannot serve the kind fails startup rather than every
    /// request.
    #[test]
    fn a_declared_block_on_a_provider_that_cannot_serve_the_kind_fails() {
        let mut cfg = config("openai", "gpt-4o");
        cfg.providers.insert("deepseek".into(), Default::default());
        cfg.model_roles.embedding.model = "deepseek/text-embedding".into();

        let err = roles(&cfg).unwrap_err();
        assert!(err.to_string().contains("serves no embedding API"), "{err}");
    }

    /// The same config through the whole table is refused: the one role the rebuild needs resolves
    /// on its own, with no chat model configured at all.
    #[test]
    fn the_embedding_role_resolves_without_a_chat_model() {
        let mut cfg = AppConfig::default();
        cfg.model_roles.embedding = EmbeddingConfig {
            model: "openai/text-embedding-3-small".into(),
            dimension: None,
        };
        cfg.providers.insert("openai".into(), Default::default());

        let registry = registry(&cfg);
        assert_eq!(
            embedding(&cfg, &registry).unwrap().base_url,
            "https://api.openai.com/v1"
        );

        let err = roles(&cfg).unwrap_err();
        assert!(err.to_string().contains("model is required"), "{err}");
    }
}
