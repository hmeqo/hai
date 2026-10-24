use serde::{Deserialize, Serialize};

use crate::{
    config::conversation::{ContentPolicy, Language},
    domain::vo::ModelAlias,
};

/// The JSON keys of the preference blob. The storage spelling is part of the existing data, so it
/// is written once here rather than at every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferenceKey {
    Language,
    Model,
    ContentPolicy,
    Persona,
}

impl PreferenceKey {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Language => "language",
            Self::Model => "model",
            Self::ContentPolicy => "content-policy",
            Self::Persona => "persona",
        }
    }
}

/// Per-conversation preferences; an absent field falls back to the configured default.
///
/// The blob is written and read by this program alone, so unknown fields are tolerated: a newer
/// build may add a preference an older build still has to read past.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ConversationPreferences {
    pub language: Option<Language>,
    pub model: Option<ModelAlias>,
    pub content_policy: Option<ContentPolicy>,
    pub persona: Option<String>,
}

impl ConversationPreferences {
    pub fn language_or(&self, default: Language) -> Language {
        self.language.unwrap_or(default)
    }

    pub fn model_or(&self, default: Option<&ModelAlias>) -> Option<ModelAlias> {
        self.model.clone().or_else(|| default.cloned())
    }

    pub fn content_policy_or(&self, default: ContentPolicy) -> ContentPolicy {
        self.content_policy.unwrap_or(default)
    }

    pub fn persona_or<'a>(&'a self, default: Option<&'a str>) -> Option<&'a str> {
        self.persona.as_deref().or(default)
    }
}

#[cfg(test)]
mod tests {
    use super::PreferenceKey;

    #[test]
    fn preference_key_spellings() {
        assert_eq!(PreferenceKey::Language.as_str(), "language");
        assert_eq!(PreferenceKey::Model.as_str(), "model");
        assert_eq!(PreferenceKey::ContentPolicy.as_str(), "content-policy");
        assert_eq!(PreferenceKey::Persona.as_str(), "persona");
    }
}
