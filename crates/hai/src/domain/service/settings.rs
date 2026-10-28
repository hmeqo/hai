//! Writes a Conversation's stored settings outside a Turn: the operator CLI path. The same
//! preferences and entries the chat commands write, with no command line and no chat gate.

use crate::{
    agentcore::ModelKind,
    config::{
        AppConfig,
        conversation::{ContentPolicy, Language},
    },
    domain::{
        repo::Repos,
        service::{ConversationRecordService, PreferenceService},
        vo::{
            ChatEntry, ChatId, ConversationId, ConversationPreferences, EntryAudience, EntryKind,
            HostAction, ModelAlias,
        },
    },
    error::{AppError, Result},
};

/// One axis the operator asked to change: a value, or a reset to the configured default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsChange<T> {
    Set(T),
    Reset,
}

/// The one reset spelling the chat command parser and the CLI share.
pub(crate) const RESET_FLAG: &str = "reset";
pub(crate) const RESET_ARG: &str = "--reset";

/// One setting an operator asked to change; the CLI and the service agree on exactly one axis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingChange {
    Language(SettingsChange<Language>),
    Model(SettingsChange<ModelAlias>),
    ContentPolicy(SettingsChange<ContentPolicy>),
    Persona(SettingsChange<String>),
}

#[derive(Debug)]
pub struct SettingsService {
    repos: Repos,
}

impl SettingsService {
    pub fn new(repos: Repos) -> Self {
        Self { repos }
    }

    /// Applies one requested setting, records its `HostAction` when the stored preference actually
    /// changed, and appends `PolicyChanged` when the effective level became stricter. A value equal
    /// to the stored preference (or a reset with nothing stored) is a no-op: it writes nothing and
    /// records nothing.
    ///
    /// Every requested value is validated before the first write; a database failure after a write
    /// keeps that write and returns the error.
    pub async fn apply(
        &self,
        config: &AppConfig,
        conversation_id: ConversationId,
        origin: ChatId,
        change: SettingChange,
    ) -> Result<ConversationPreferences> {
        let change = normalize(config, change)?;

        let preferences = PreferenceService::new(self.repos.clone());
        let records = ConversationRecordService::new(self.repos.clone());
        let before = preferences.get(conversation_id).await?.unwrap_or_default();
        let old_policy = before
            .content_policy
            .unwrap_or(config.conversation.content_policy);

        let (action, new_policy) = match change {
            SettingChange::Language(SettingsChange::Set(language))
                if before.language != Some(language) =>
            {
                preferences.set_language(conversation_id, language).await?;
                (Some(HostAction::LanguageChanged), None)
            }
            SettingChange::Language(SettingsChange::Reset) if before.language.is_some() => {
                preferences.reset_language(conversation_id).await?;
                (Some(HostAction::LanguageChanged), None)
            }
            SettingChange::Language(_) => (None, None),
            SettingChange::Model(SettingsChange::Set(model))
                if before.model.as_ref().map(ModelAlias::as_str) != Some(model.as_str()) =>
            {
                preferences.set_model(conversation_id, model).await?;
                (Some(HostAction::ModelChanged), None)
            }
            SettingChange::Model(SettingsChange::Reset) if before.model.is_some() => {
                preferences.reset_model(conversation_id).await?;
                (Some(HostAction::ModelChanged), None)
            }
            SettingChange::Model(_) => (None, None),
            SettingChange::ContentPolicy(SettingsChange::Set(policy))
                if before.content_policy != Some(policy) =>
            {
                preferences
                    .set_content_policy(conversation_id, policy)
                    .await?;
                (Some(HostAction::ContentPolicyChanged), Some(policy))
            }
            SettingChange::ContentPolicy(SettingsChange::Reset)
                if before.content_policy.is_some() =>
            {
                preferences.reset_content_policy(conversation_id).await?;
                (
                    Some(HostAction::ContentPolicyChanged),
                    Some(config.conversation.content_policy),
                )
            }
            SettingChange::ContentPolicy(_) => (None, None),
            SettingChange::Persona(SettingsChange::Set(text))
                if before.persona.as_deref() != Some(text.as_str()) =>
            {
                preferences.set_persona(conversation_id, text).await?;
                (Some(HostAction::PersonaChanged), None)
            }
            SettingChange::Persona(SettingsChange::Reset) if before.persona.is_some() => {
                preferences.reset_persona(conversation_id).await?;
                (Some(HostAction::PersonaChanged), None)
            }
            SettingChange::Persona(_) => (None, None),
        };

        if let Some(action) = action {
            records
                .record_host_action(conversation_id, origin, action)
                .await?;
        }
        if new_policy.is_some_and(|new| new.is_stricter_than(old_policy)) {
            let entry = ChatEntry::new(
                origin,
                EntryAudience::Agent,
                EntryKind::PolicyChanged,
                jiff::Timestamp::now(),
            );
            records.append_entry(conversation_id, &entry).await?;
        }

        Ok(preferences.get(conversation_id).await?.unwrap_or_default())
    }
}

/// Trims and validates the values that need it before any database write.
fn normalize(config: &AppConfig, change: SettingChange) -> Result<SettingChange> {
    match change {
        SettingChange::Model(SettingsChange::Set(alias)) => {
            validate_chat_alias(config, alias.as_str())?;
            Ok(SettingChange::Model(SettingsChange::Set(alias)))
        }
        SettingChange::Persona(SettingsChange::Set(text)) => {
            let text = text.trim().to_owned();
            validate_persona(&text)?;
            Ok(SettingChange::Persona(SettingsChange::Set(text)))
        }
        other => Ok(other),
    }
}

/// A model preference names a `[models]` alias that fills the chat role; a direct `provider/model`
/// is not a stored value.
fn validate_chat_alias(config: &AppConfig, alias: &str) -> Result<()> {
    let aliases: Vec<&str> = config
        .models
        .iter()
        .filter(|(_, model)| model.roles.contains(&ModelKind::Chat))
        .map(|(alias, _)| alias.as_str())
        .collect();
    match config.models.get(alias) {
        None if aliases.is_empty() => {
            Err(AppError::message("no [models] entry declares role 'chat'"))
        }
        None => Err(AppError::message(format!(
            "unknown model alias {alias:?}; chat aliases: {}",
            aliases.join(", ")
        ))),
        Some(model) if !model.roles.contains(&ModelKind::Chat) => Err(AppError::message(format!(
            "model alias {alias:?} does not declare role 'chat'"
        ))),
        Some(_) => Ok(()),
    }
}

fn validate_persona(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Err(AppError::message("persona must not be empty"));
    }
    if text.chars().count() > 4096 {
        return Err(AppError::message("persona is longer than 4096 characters"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{RESET_ARG, RESET_FLAG, validate_chat_alias, validate_persona};
    use crate::{
        agentcore::ModelKind,
        config::{
            AppConfig,
            schema::{InputModality, ModelConfig},
        },
    };

    fn model(model: &str, role: ModelKind) -> ModelConfig {
        ModelConfig {
            model: format!("openai/{model}"),
            name: None,
            roles: BTreeSet::from([role]),
            input: BTreeSet::from([InputModality::Text]),
            max_tokens: None,
            context_window: None,
            temperature: None,
            thinking: None,
            pricing: None,
        }
    }

    fn config_with_chat_and_vision() -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.models
            .insert("main".into(), model("gpt", ModelKind::Chat));
        cfg.models
            .insert("eyes".into(), model("vision", ModelKind::Vision));
        cfg
    }

    #[test]
    fn reset_spellings_agree() {
        assert_eq!(RESET_ARG.strip_prefix("--"), Some(RESET_FLAG));
    }

    #[test]
    fn a_chat_alias_is_accepted() {
        validate_chat_alias(&config_with_chat_and_vision(), "main").expect("chat alias");
    }

    #[test]
    fn an_unknown_alias_lists_the_chat_aliases() {
        let error =
            validate_chat_alias(&config_with_chat_and_vision(), "nope").expect_err("unknown alias");
        assert_eq!(
            error.to_string(),
            "unknown model alias \"nope\"; chat aliases: main"
        );
    }

    #[test]
    fn an_alias_without_the_chat_role_is_rejected() {
        let error = validate_chat_alias(&config_with_chat_and_vision(), "eyes")
            .expect_err("non-chat alias");
        assert_eq!(
            error.to_string(),
            "model alias \"eyes\" does not declare role 'chat'"
        );
    }

    #[test]
    fn no_chat_alias_at_all_is_its_own_error() {
        let error = validate_chat_alias(&AppConfig::default(), "main").expect_err("no chat model");
        assert_eq!(error.to_string(), "no [models] entry declares role 'chat'");
    }

    #[test]
    fn persona_is_trimmed_and_bounded() {
        validate_persona("a persona").expect("valid persona");
        validate_persona(&"x".repeat(4096)).expect("4096 characters");
        assert_eq!(
            validate_persona("  ")
                .expect_err("blank persona")
                .to_string(),
            "persona must not be empty"
        );
        assert_eq!(
            validate_persona(&"x".repeat(4097))
                .expect_err("too long")
                .to_string(),
            "persona is longer than 4096 characters"
        );
    }
}
