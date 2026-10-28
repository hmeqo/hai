use std::sync::Arc;

use crate::{
    agent::command::CommandGates,
    config::{
        AppConfig,
        conversation::{ContentPolicy, Language},
    },
    domain::{
        service::Services,
        vo::{ConversationId, ConversationPreferences, ModelAlias},
    },
    error::Result,
};

/// The settings a Conversation runs with: the configured defaults, overridden by its own stored
/// preference where one exists. Absence is a value — a reset restores the default.
#[derive(Debug, Clone)]
pub struct ResolvedConversationSettings {
    pub language: Language,
    pub model: Option<ModelAlias>,
    pub content_policy: ContentPolicy,
    pub persona: Option<String>,
}

/// Resolves one stored preference against the configured defaults. Absence of a preference and an
/// absent configured persona both land as `None`.
pub(crate) fn resolve(
    cfg: &AppConfig,
    prefs: &ConversationPreferences,
) -> ResolvedConversationSettings {
    ResolvedConversationSettings {
        language: prefs.language_or(cfg.conversation.language),
        model: prefs.model.clone(),
        content_policy: prefs.content_policy_or(cfg.conversation.content_policy),
        persona: prefs
            .persona_or(cfg.conversation.persona.as_deref())
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned),
    }
}

#[derive(Clone)]
pub struct ConversationSettings {
    cfg: Arc<AppConfig>,
    services: Services,
}

impl ConversationSettings {
    pub fn new(cfg: Arc<AppConfig>, services: Services) -> Self {
        Self { cfg, services }
    }

    /// One read of the preference row, resolved against the configured defaults.
    pub async fn snapshot(
        &self,
        conversation_id: ConversationId,
    ) -> Result<ResolvedConversationSettings> {
        let prefs = self
            .services
            .preference
            .get(conversation_id)
            .await?
            .unwrap_or_default();
        Ok(resolve(self.cfg.as_ref(), &prefs))
    }

    pub async fn language(&self, conversation_id: ConversationId) -> Result<Language> {
        let prefs = self.services.preference.get(conversation_id).await?;
        Ok(prefs
            .unwrap_or_default()
            .language_or(self.cfg.conversation.language))
    }

    pub async fn set_language(
        &self,
        conversation_id: ConversationId,
        language: Language,
    ) -> Result<()> {
        self.services
            .preference
            .set_language(conversation_id, language)
            .await
    }

    pub async fn reset_language(&self, conversation_id: ConversationId) -> Result<()> {
        self.services
            .preference
            .reset_language(conversation_id)
            .await
    }

    /// The Conversation's stored model; `None` means `[model-roles.default]`.
    pub async fn model(&self, conversation_id: ConversationId) -> Result<Option<ModelAlias>> {
        let prefs = self.services.preference.get(conversation_id).await?;
        Ok(prefs.unwrap_or_default().model)
    }

    pub async fn reset_model(&self, conversation_id: ConversationId) -> Result<()> {
        self.services.preference.reset_model(conversation_id).await
    }

    pub async fn content_policy(&self, conversation_id: ConversationId) -> Result<ContentPolicy> {
        let prefs = self.services.preference.get(conversation_id).await?;
        Ok(prefs
            .unwrap_or_default()
            .content_policy_or(self.cfg.conversation.content_policy))
    }

    pub async fn set_content_policy(
        &self,
        conversation_id: ConversationId,
        policy: ContentPolicy,
    ) -> Result<()> {
        self.services
            .preference
            .set_content_policy(conversation_id, policy)
            .await
    }

    pub async fn reset_content_policy(&self, conversation_id: ConversationId) -> Result<()> {
        self.services
            .preference
            .reset_content_policy(conversation_id)
            .await
    }

    pub async fn persona(&self, conversation_id: ConversationId) -> Result<Option<String>> {
        let prefs = self.services.preference.get(conversation_id).await?;
        Ok(prefs
            .unwrap_or_default()
            .persona_or(self.cfg.conversation.persona.as_deref())
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned))
    }

    pub async fn set_persona(
        &self,
        conversation_id: ConversationId,
        persona: String,
    ) -> Result<()> {
        self.services
            .preference
            .set_persona(conversation_id, persona)
            .await
    }

    pub async fn reset_persona(&self, conversation_id: ConversationId) -> Result<()> {
        self.services
            .preference
            .reset_persona(conversation_id)
            .await
    }

    pub fn command_gates(&self) -> CommandGates {
        CommandGates {
            allow_persona_change: self.cfg.conversation.allow_persona_change,
            allow_content_policy_change: self.cfg.conversation.allow_content_policy_change,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::resolve;
    use crate::{
        config::{
            AppConfig,
            conversation::{ContentPolicy, Language},
        },
        domain::vo::{ConversationPreferences, ModelAlias},
    };

    fn configured() -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.conversation.language = Language::En;
        cfg.conversation.content_policy = ContentPolicy::Standard;
        cfg.conversation.persona = Some("configured persona".into());
        cfg
    }

    #[test]
    fn absent_preferences_resolve_to_the_configured_defaults() {
        let resolved = resolve(&configured(), &ConversationPreferences::default());
        assert_eq!(resolved.language, Language::En);
        assert_eq!(resolved.model, None);
        assert_eq!(resolved.content_policy, ContentPolicy::Standard);
        assert_eq!(resolved.persona.as_deref(), Some("configured persona"));
    }

    #[test]
    fn stored_preferences_override_the_defaults() {
        let prefs = ConversationPreferences {
            language: Some(Language::Zh),
            model: Some(ModelAlias::new("main")),
            content_policy: Some(ContentPolicy::Unrestricted),
            persona: Some("stored persona".into()),
        };
        let resolved = resolve(&configured(), &prefs);
        assert_eq!(resolved.language, Language::Zh);
        assert_eq!(resolved.model, Some(ModelAlias::new("main")));
        assert_eq!(resolved.content_policy, ContentPolicy::Unrestricted);
        assert_eq!(resolved.persona.as_deref(), Some("stored persona"));
    }

    #[test]
    fn a_blank_stored_persona_does_not_fall_back_to_the_configured_one() {
        let prefs = ConversationPreferences {
            persona: Some("  ".into()),
            ..ConversationPreferences::default()
        };
        let resolved = resolve(&configured(), &prefs);
        assert_eq!(resolved.persona, None);
    }
}
