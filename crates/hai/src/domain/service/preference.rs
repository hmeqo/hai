use crate::{
    config::conversation::{ContentPolicy, Language},
    domain::{
        repo::Repos,
        vo::{ConversationId, ConversationPreferences, ModelAlias, PreferenceKey},
    },
    error::{AppError, Result},
};

#[derive(Debug)]
pub struct PreferenceService {
    repos: Repos,
}

impl PreferenceService {
    pub(crate) fn new(repos: Repos) -> Self {
        Self { repos }
    }

    /// Absence is a value: the conversation inherits the configured default. A stored blob that
    /// cannot be read is an error — returning absence would let the next write overwrite it.
    pub async fn get(
        &self,
        conversation_id: ConversationId,
    ) -> Result<Option<ConversationPreferences>> {
        let Some(row) = self
            .repos
            .conversation_preference
            .get(conversation_id)
            .await?
        else {
            return Ok(None);
        };
        parse_blob(conversation_id, row.prefs).map(Some)
    }

    async fn set(
        &self,
        conversation_id: ConversationId,
        key: PreferenceKey,
        value: impl serde::Serialize,
    ) -> Result<()> {
        self.repos
            .conversation_preference
            .set_key(conversation_id, key, &serde_json::to_value(value)?)
            .await
    }

    pub async fn set_language(
        &self,
        conversation_id: ConversationId,
        language: Language,
    ) -> Result<()> {
        self.set(conversation_id, PreferenceKey::Language, language)
            .await
    }

    pub async fn set_model(
        &self,
        conversation_id: ConversationId,
        model: ModelAlias,
    ) -> Result<()> {
        self.set(conversation_id, PreferenceKey::Model, model).await
    }

    pub async fn set_content_policy(
        &self,
        conversation_id: ConversationId,
        policy: ContentPolicy,
    ) -> Result<()> {
        self.set(conversation_id, PreferenceKey::ContentPolicy, policy)
            .await
    }

    pub async fn set_persona(
        &self,
        conversation_id: ConversationId,
        persona: String,
    ) -> Result<()> {
        self.set(conversation_id, PreferenceKey::Persona, persona)
            .await
    }

    async fn clear(&self, conversation_id: ConversationId, key: PreferenceKey) -> Result<()> {
        self.repos
            .conversation_preference
            .remove_key(conversation_id, key)
            .await
    }

    pub async fn reset_language(&self, conversation_id: ConversationId) -> Result<()> {
        self.clear(conversation_id, PreferenceKey::Language).await
    }

    pub async fn reset_model(&self, conversation_id: ConversationId) -> Result<()> {
        self.clear(conversation_id, PreferenceKey::Model).await
    }

    pub async fn reset_content_policy(&self, conversation_id: ConversationId) -> Result<()> {
        self.clear(conversation_id, PreferenceKey::ContentPolicy)
            .await
    }

    pub async fn reset_persona(&self, conversation_id: ConversationId) -> Result<()> {
        self.clear(conversation_id, PreferenceKey::Persona).await
    }
}

/// A stored blob that does not decode is an error: absence means "no row", not "unreadable row".
fn parse_blob(
    conversation_id: ConversationId,
    value: serde_json::Value,
) -> Result<ConversationPreferences> {
    serde_json::from_value(value).map_err(|e| {
        AppError::message(format!(
            "conversation {conversation_id} has unreadable preferences: {e}"
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unreadable_blob_is_an_error_not_absence() {
        let err = parse_blob(ConversationId(3), serde_json::json!(5)).unwrap_err();
        assert!(err.to_string().contains("unreadable preferences"), "{err}");
    }
}
