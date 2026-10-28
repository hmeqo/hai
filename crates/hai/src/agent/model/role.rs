//! The `[model-roles]` table: every role resolves through a `RolePlan` against the catalog.

use std::collections::BTreeMap;

use super::{ModelCatalog, ModelEntry, provider::ProviderRegistry};
use crate::{
    agentcore::{ModelKind, ModelTarget, provider::ChatTransport},
    config::{AppConfig, ConfigIssue},
    error::Result,
};

/// What a role block asks for: one alias, the only declared candidate, or nothing.
#[derive(Debug, Clone, Copy)]
enum RoleChoice<'a> {
    Explicit(&'a str),
    Auto,
    Off,
}

/// Whether zero candidates are an error or simply turn the role off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Requirement {
    Required,
    Optional,
}

/// One role's decoded config block, before it touches the provider registry.
#[derive(Debug, Clone, Copy)]
struct RolePlan<'a> {
    kind: ModelKind,
    choice: RoleChoice<'a>,
    requirement: Requirement,
}

impl<'a> RolePlan<'a> {
    fn from_config(config: &'a AppConfig, kind: ModelKind) -> Self {
        let roles = &config.model_roles;
        match kind {
            ModelKind::Chat => Self {
                kind,
                choice: role_choice(Some(&roles.default.model)),
                requirement: Requirement::Required,
            },
            ModelKind::Vision => {
                optional_plan(kind, roles.vision.as_ref().map(|b| b.model.as_deref()))
            }
            ModelKind::Transcription => optional_plan(
                kind,
                roles.transcription.as_ref().map(|b| b.model.as_deref()),
            ),
            ModelKind::Speech => {
                optional_plan(kind, roles.speech.as_ref().map(|b| b.model.as_deref()))
            }
            ModelKind::Image => {
                optional_plan(kind, roles.image.as_ref().map(|b| b.model.as_deref()))
            }
            ModelKind::Embedding => Self {
                kind,
                choice: role_choice(Some(&roles.embedding.model)),
                requirement: Requirement::Required,
            },
            ModelKind::Rerank => {
                optional_plan(kind, roles.rerank.as_ref().map(|b| b.model.as_deref()))
            }
        }
    }
}

/// Every role block's selector, with the kind that block fills; an empty alias (auto-select) names
/// no selector and yields nothing.
pub(crate) fn selectors(config: &AppConfig) -> impl Iterator<Item = (&str, ModelKind)> + '_ {
    let roles = &config.model_roles;
    [
        Some((roles.default.model.as_str(), ModelKind::Chat)),
        roles
            .vision
            .as_ref()
            .and_then(|block| block.model.as_deref())
            .map(|selector| (selector, ModelKind::Vision)),
        roles
            .transcription
            .as_ref()
            .and_then(|block| block.model.as_deref())
            .map(|selector| (selector, ModelKind::Transcription)),
        roles
            .speech
            .as_ref()
            .and_then(|block| block.model.as_deref())
            .map(|selector| (selector, ModelKind::Speech)),
        roles
            .image
            .as_ref()
            .and_then(|block| block.model.as_deref())
            .map(|selector| (selector, ModelKind::Image)),
        Some((roles.embedding.model.as_str(), ModelKind::Embedding)),
        roles
            .rerank
            .as_ref()
            .and_then(|block| block.model.as_deref())
            .map(|selector| (selector, ModelKind::Rerank)),
    ]
    .into_iter()
    .flatten()
    .filter(|(selector, _)| !selector.trim().is_empty())
}

/// The selectors that spell `provider/model` themselves; an alias is resolved through its `[models]`
/// entry.
pub(crate) fn direct_selectors(config: &AppConfig) -> impl Iterator<Item = (&str, ModelKind)> + '_ {
    selectors(config).filter(|(selector, _)| selector.contains('/'))
}

fn role_choice(alias: Option<&str>) -> RoleChoice<'_> {
    alias
        .filter(|alias| !alias.trim().is_empty())
        .map(RoleChoice::Explicit)
        .unwrap_or(RoleChoice::Auto)
}

fn optional_plan(kind: ModelKind, block: Option<Option<&str>>) -> RolePlan<'_> {
    match block {
        Some(alias) => RolePlan {
            kind,
            choice: role_choice(alias),
            requirement: Requirement::Optional,
        },
        None => RolePlan {
            kind,
            choice: RoleChoice::Off,
            requirement: Requirement::Optional,
        },
    }
}

/// Declared candidates grouped once, so the resolver never scans the whole catalog per role.
struct CapabilityIndex<'a> {
    by_kind: BTreeMap<ModelKind, Vec<&'a ModelEntry>>,
}

impl<'a> CapabilityIndex<'a> {
    fn new(catalog: &'a ModelCatalog) -> Self {
        let mut by_kind: BTreeMap<ModelKind, Vec<&ModelEntry>> = BTreeMap::new();
        for entry in catalog.entries() {
            for kind in &entry.config.roles {
                by_kind.entry(*kind).or_default().push(entry);
            }
        }
        Self { by_kind }
    }

    fn candidates(&self, kind: ModelKind) -> &[&'a ModelEntry] {
        self.by_kind.get(&kind).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// Pure role resolution: plan + catalog + provider registry in, endpoint out.
struct RoleResolver<'a, 'r> {
    catalog: &'a ModelCatalog,
    index: CapabilityIndex<'a>,
    registry: &'r ProviderRegistry,
}

impl<'a, 'r> RoleResolver<'a, 'r> {
    fn new(catalog: &'a ModelCatalog, registry: &'r ProviderRegistry) -> Self {
        Self {
            catalog,
            index: CapabilityIndex::new(catalog),
            registry,
        }
    }

    fn entry(&self, plan: RolePlan<'_>) -> Result<Option<&'a ModelEntry>> {
        match plan.choice {
            RoleChoice::Explicit(alias) => self.explicit(plan.kind, alias).map(Some),
            RoleChoice::Auto => self.unique(plan.kind, plan.requirement),
            RoleChoice::Off => Ok(None),
        }
    }

    fn target(&self, plan: RolePlan<'_>) -> Result<Option<ModelTarget>> {
        self.entry(plan)?
            .map(|entry| self.endpoint(plan.kind, entry))
            .transpose()
    }

    fn required_target(&self, plan: RolePlan<'_>) -> Result<ModelTarget> {
        let entry = self
            .entry(plan)?
            .expect("a required role always resolves to a model");
        self.endpoint(plan.kind, entry)
    }

    fn explicit(&self, kind: ModelKind, alias: &str) -> Result<&'a ModelEntry> {
        let entry = self
            .catalog
            .get(alias)
            .ok_or_else(|| missing_alias(kind, alias))?;
        if !entry.supports(kind) {
            return Err(not_a_role(kind, alias).into());
        }
        Ok(entry)
    }

    fn unique(&self, kind: ModelKind, requirement: Requirement) -> Result<Option<&'a ModelEntry>> {
        let candidates = self.index.candidates(kind);
        match candidates {
            [] if requirement == Requirement::Required => {
                Err(ConfigIssue::model_role(kind, model_required(kind)).into())
            }
            [] => Ok(None),
            [entry] => Ok(Some(entry)),
            many => {
                let aliases: Vec<&str> = many.iter().map(|entry| entry.alias.as_str()).collect();
                Err(ConfigIssue::model_role(
                    kind,
                    format!(
                        "multiple [models] entries declare role '{kind}': {}; set the alias explicitly",
                        aliases.join(", ")
                    ),
                )
                .into())
            }
        }
    }

    fn endpoint(&self, kind: ModelKind, entry: &ModelEntry) -> Result<ModelTarget> {
        let identity = entry.identity();
        self.registry
            .endpoint(kind, identity.provider, identity.model, None)
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

/// Resolves the whole table against the catalog and the providers this process can reach.
pub(crate) fn resolve(
    config: &AppConfig,
    registry: &ProviderRegistry,
    catalog: &ModelCatalog,
) -> Result<ModelRoles> {
    let resolver = RoleResolver::new(catalog, registry);
    let chat_entry = resolver
        .entry(RolePlan::from_config(config, ModelKind::Chat))?
        .expect("a required role always resolves to a model");
    let chat = chat_entry
        .chat
        .clone()
        .expect("a chat entry has a transport");
    Ok(ModelRoles {
        chat,
        vision: resolver.target(RolePlan::from_config(config, ModelKind::Vision))?,
        transcription: resolver.target(RolePlan::from_config(config, ModelKind::Transcription))?,
        speech: resolver.target(RolePlan::from_config(config, ModelKind::Speech))?,
        image: resolver.target(RolePlan::from_config(config, ModelKind::Image))?,
        embedding: resolver.required_target(RolePlan::from_config(config, ModelKind::Embedding))?,
        rerank: resolver.target(RolePlan::from_config(config, ModelKind::Rerank))?,
    })
}

/// The embedding role, resolved without the rest of the table.
pub(crate) fn embedding(
    config: &AppConfig,
    registry: &ProviderRegistry,
    catalog: &ModelCatalog,
) -> Result<ModelTarget> {
    let resolver = RoleResolver::new(catalog, registry);
    resolver.required_target(RolePlan::from_config(config, ModelKind::Embedding))
}

fn missing_alias(kind: ModelKind, alias: &str) -> ConfigIssue {
    ConfigIssue::model_role(
        kind,
        format!("model alias '{alias}' is not declared in [models]"),
    )
}

fn not_a_role(kind: ModelKind, alias: &str) -> ConfigIssue {
    ConfigIssue::model_role(
        kind,
        format!("model '{alias}' does not declare role '{kind}'"),
    )
}

/// The message a present block earns when it names no alias.
fn model_required(kind: ModelKind) -> &'static str {
    match kind {
        ModelKind::Embedding => "is required (memory/retrieval depends on it) but model is empty",
        _ => "model is required (a [models] alias)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::test_support::{chat_embedding_config, model_config},
        config::schema::{
            EmbeddingConfig, ImageConfig, ProviderConfig, RerankConfig, VisionConfig,
        },
    };

    /// The roles these tests exercise are about resolution, not availability, so every provider is
    /// given a credential and every endpoint answers.
    fn registry(config: &AppConfig) -> ProviderRegistry {
        ProviderRegistry::from_sources(config, &|_| Some("test-key".to_owned()), &|_| true)
    }

    fn config(provider: &str, model_name: &str, roles: Vec<ModelKind>) -> AppConfig {
        let mut cfg = chat_embedding_config(provider, model_name);
        cfg.models.get_mut("main").unwrap().roles = roles.into_iter().collect();
        cfg
    }

    fn roles(cfg: &AppConfig) -> Result<ModelRoles> {
        super::resolve(cfg, &registry(cfg), &catalog(cfg)?)
    }

    fn catalog(cfg: &AppConfig) -> Result<ModelCatalog> {
        ModelCatalog::resolve(cfg, &registry(cfg))
    }

    /// An absent optional block is off; nothing is selected implicitly.
    #[test]
    fn an_absent_optional_block_is_off() {
        let cfg = config("anthropic", "claude-sonnet-4-5", vec![ModelKind::Chat]);

        let roles = roles(&cfg).unwrap();
        assert!(roles.vision.is_none());
        assert!(roles.transcription.is_none());
        assert!(roles.image.is_none());
        assert!(roles.speech.is_none());
        assert!(roles.rerank.is_none());
    }

    /// A block that names an alias runs on the entry's provider, which is where its endpoint comes
    /// from too.
    #[test]
    fn a_named_alias_uses_its_entry_provider() {
        let mut cfg = config("openrouter", "openai/gpt-4o", vec![ModelKind::Chat]);
        cfg.providers.insert(
            "openrouter".into(),
            ProviderConfig {
                base_url: Some("https://gateway.internal/v1".into()),
                ..Default::default()
            },
        );
        cfg.models.insert(
            "image-model".into(),
            model_config("openrouter", "some-image-model", [ModelKind::Image]),
        );
        cfg.model_roles.image = Some(ImageConfig {
            model: Some("image-model".into()),
        });

        let roles = roles(&cfg).unwrap();
        assert_eq!(roles.image.unwrap().base_url, "https://gateway.internal/v1");
    }

    /// A present block with no declared candidate is simply off.
    #[test]
    fn a_present_block_without_a_candidate_is_off() {
        let mut cfg = config("openai", "gpt-4o", vec![ModelKind::Chat]);
        cfg.model_roles.image = Some(ImageConfig { model: None });

        let roles = roles(&cfg).unwrap();
        assert!(roles.image.is_none());
    }

    /// A rerank alias keeps its provider's own endpoint.
    #[test]
    fn a_rerank_alias_takes_the_entry_provider_and_the_kinds_own_endpoint() {
        let mut cfg = config("openrouter", "openai/gpt-4o", vec![ModelKind::Chat]);
        cfg.models.insert(
            "reranker".into(),
            model_config("openrouter", "cohere/rerank-4-fast", [ModelKind::Rerank]),
        );
        cfg.model_roles.rerank = Some(RerankConfig {
            model: Some("reranker".into()),
        });

        let roles = roles(&cfg).unwrap();
        assert_eq!(
            roles.rerank.unwrap().base_url,
            "https://openrouter.ai/api/v1/rerank"
        );
    }

    /// A direct selector works without a `[models]` preset.
    #[test]
    fn a_direct_default_selector_needs_no_preset() {
        let mut cfg = config("openai", "gpt-4o", vec![ModelKind::Chat]);
        cfg.models.remove("main");
        cfg.model_roles.default.model = "openai/gpt-4o".into();

        let roles = roles(&cfg).unwrap();
        assert_eq!(roles.chat.target.model, "gpt-4o");
    }

    /// A single chat candidate is selected when the default alias is empty.
    #[test]
    fn a_single_chat_candidate_is_selected() {
        let mut cfg = config("openai", "gpt-4o", vec![ModelKind::Chat]);
        cfg.model_roles.default.model = String::new();

        let roles = roles(&cfg).unwrap();
        assert_eq!(roles.chat.target.model, "gpt-4o");
    }

    /// Multiple chat candidates require an explicit default.
    #[test]
    fn multiple_chat_candidates_are_ambiguous() {
        let mut cfg = config("openai", "gpt-4o", vec![ModelKind::Chat]);
        cfg.models.insert(
            "other-chat".into(),
            model_config("openai", "gpt-4.1", [ModelKind::Chat]),
        );
        cfg.model_roles.default.model = String::new();

        let err = roles(&cfg).unwrap_err();
        assert!(
            err.to_string()
                .contains("multiple [models] entries declare role 'chat'"),
            "{err}"
        );
    }

    /// A single embedding candidate is selected when its alias is empty.
    #[test]
    fn a_single_embedding_candidate_is_selected() {
        let mut cfg = config("openai", "gpt-4o", vec![ModelKind::Chat]);
        cfg.model_roles.embedding.model = String::new();

        let roles = roles(&cfg).unwrap();
        assert_eq!(roles.embedding.model, "text-embedding-3-small");
    }

    /// A present optional block selects its only declared candidate.
    #[test]
    fn a_present_block_selects_the_only_candidate() {
        let mut cfg = config("openai", "gpt-4o", vec![ModelKind::Chat]);
        cfg.models.insert(
            "vision-model".into(),
            model_config("openai", "gpt-4o-vision", [ModelKind::Vision]),
        );
        cfg.model_roles.vision = Some(VisionConfig { model: None });

        let roles = roles(&cfg).unwrap();
        assert_eq!(roles.vision.unwrap().model, "gpt-4o-vision");
    }

    /// Multiple candidates for an optional role are an error.
    #[test]
    fn multiple_optional_candidates_are_ambiguous() {
        let mut cfg = config("openai", "gpt-4o", vec![ModelKind::Chat]);
        cfg.models.insert(
            "vision-a".into(),
            model_config("openai", "gpt-4o-vision", [ModelKind::Vision]),
        );
        cfg.models.insert(
            "vision-b".into(),
            model_config("openai", "gpt-4.1-vision", [ModelKind::Vision]),
        );
        cfg.model_roles.vision = Some(VisionConfig { model: None });

        let err = roles(&cfg).unwrap_err();
        assert!(
            err.to_string()
                .contains("multiple [models] entries declare role 'vision'"),
            "{err}"
        );
    }

    /// A declared alias that is absent from `[models]` names the role block that needs it.
    #[test]
    fn a_missing_alias_names_the_role() {
        let mut cfg = config("openai", "gpt-4o", vec![ModelKind::Chat]);
        cfg.model_roles.image = Some(ImageConfig {
            model: Some("absent".into()),
        });

        let err = roles(&cfg).unwrap_err();
        assert!(err.to_string().contains("[model-roles.image]"), "{err}");
        assert!(err.to_string().contains("absent"), "{err}");
    }

    /// A declared alias whose entry omits the role is a config error, not a silent fallback.
    #[test]
    fn a_role_the_model_does_not_declare_fails() {
        let mut cfg = config("openai", "gpt-4o", vec![ModelKind::Chat]);
        cfg.model_roles.image = Some(ImageConfig {
            model: Some("main".into()),
        });

        let err = roles(&cfg).unwrap_err();
        assert!(
            err.to_string().contains("does not declare role 'image'"),
            "{err}"
        );
    }

    /// A declared block on a provider that cannot serve the kind fails startup rather than every
    /// request.
    #[test]
    fn a_declared_block_on_a_provider_that_cannot_serve_the_kind_fails() {
        let mut cfg = config("openai", "gpt-4o", vec![ModelKind::Chat]);
        cfg.providers.insert("deepseek".into(), Default::default());
        cfg.model_roles.embedding.model = "deepseek-embed".into();
        cfg.models.insert(
            "deepseek-embed".into(),
            model_config("deepseek", "text-embedding", [ModelKind::Embedding]),
        );

        let err = roles(&cfg).unwrap_err();
        assert!(err.to_string().contains("serves no embedding API"), "{err}");
    }

    /// The same config through the whole table is refused: the one role the rebuild needs resolves
    /// on its own, with no chat model configured at all.
    #[test]
    fn the_embedding_role_resolves_without_a_chat_model() {
        let mut cfg = AppConfig::default();
        cfg.model_roles.embedding = EmbeddingConfig {
            model: "embed".into(),
            dimension: None,
        };
        cfg.models.insert(
            "embed".into(),
            model_config("openai", "text-embedding-3-small", [ModelKind::Embedding]),
        );
        cfg.providers.insert("openai".into(), Default::default());

        let registry = registry(&cfg);
        let catalog = catalog(&cfg).unwrap();
        assert_eq!(
            embedding(&cfg, &registry, &catalog).unwrap().base_url,
            "https://api.openai.com/v1"
        );

        let err = roles(&cfg).unwrap_err();
        assert!(err.to_string().contains("model is required"), "{err}");
    }
}
