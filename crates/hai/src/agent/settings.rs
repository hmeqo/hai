use std::sync::Arc;

use crate::{
    agent::command::CommandGates,
    config::{
        AppConfig,
        conversation::{ContentPolicy, Language},
    },
    domain::{
        service::Services,
        vo::{ConversationId, ModelAlias},
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
        let default_model = self.cfg.conversation.model.as_deref().map(ModelAlias::new);
        Ok(ResolvedConversationSettings {
            language: prefs.language_or(self.cfg.conversation.language),
            model: prefs.model_or(default_model.as_ref()),
            content_policy: prefs.content_policy_or(self.cfg.conversation.content_policy),
            persona: prefs
                .persona_or(self.cfg.conversation.persona.as_deref())
                .filter(|text| !text.trim().is_empty())
                .map(str::to_owned),
        })
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

    /// The stored alias, or the configured default; `None` means `[model-roles.default]`.
    pub async fn model(&self, conversation_id: ConversationId) -> Result<Option<ModelAlias>> {
        let prefs = self.services.preference.get(conversation_id).await?;
        Ok(prefs.unwrap_or_default().model_or(
            self.cfg
                .conversation
                .model
                .as_deref()
                .map(ModelAlias::new)
                .as_ref(),
        ))
    }

    pub async fn set_model(
        &self,
        conversation_id: ConversationId,
        model: ModelAlias,
    ) -> Result<()> {
        self.services
            .preference
            .set_model(conversation_id, model)
            .await
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

    /// The alias in `[conversation].model`, without reading the Conversation's preference.
    pub fn configured_model(&self) -> Option<ModelAlias> {
        self.cfg.conversation.model.as_deref().map(ModelAlias::new)
    }

    pub fn model_aliases(&self) -> impl Iterator<Item = &str> {
        self.cfg.models.keys().map(String::as_str)
    }

    pub fn model_exists(&self, alias: &str) -> bool {
        self.cfg.models.contains_key(alias)
    }

    pub fn default_model_label(&self) -> &str {
        self.cfg
            .model_roles
            .default
            .model
            .split_once('/')
            .map(|(_, model)| model)
            .unwrap_or(&self.cfg.model_roles.default.model)
    }

    pub fn command_gates(&self) -> CommandGates {
        CommandGates {
            allow_persona_change: self.cfg.conversation.allow_persona_change,
            allow_content_policy_change: self.cfg.conversation.allow_content_policy_change,
        }
    }
}
