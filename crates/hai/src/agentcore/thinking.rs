use genai::chat::ReasoningEffort;
use serde::{Deserialize, Serialize};
use strum::{EnumIter, EnumString, IntoStaticStr};

/// A reasoning effort a model accepts. The keyword is both the config wire form and the provider
/// word; `ReasoningEffort` is the provider-side type it converts to.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    EnumIter,
    EnumString,
    IntoStaticStr,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ThinkingEffort {
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl ThinkingEffort {
    pub fn keyword(self) -> &'static str {
        self.into()
    }

    pub fn from_keyword(value: &str) -> Option<Self> {
        value.parse().ok()
    }
}

impl From<ThinkingEffort> for ReasoningEffort {
    fn from(effort: ThinkingEffort) -> Self {
        match effort {
            ThinkingEffort::Minimal => Self::Minimal,
            ThinkingEffort::Low => Self::Low,
            ThinkingEffort::Medium => Self::Medium,
            ThinkingEffort::High => Self::High,
            ThinkingEffort::XHigh => Self::XHigh,
            ThinkingEffort::Max => Self::Max,
        }
    }
}

#[cfg(test)]
mod tests {
    use strum::IntoEnumIterator;

    use super::*;

    /// The serde name, the keyword and the provider variant are one vocabulary.
    #[test]
    fn wire_form_and_keywords_agree() {
        crate::test_support::assert_names_agree(ThinkingEffort::iter(), |effort| effort.keyword());
        for effort in ThinkingEffort::iter() {
            assert_eq!(
                effort.keyword().parse::<ThinkingEffort>().ok(),
                Some(effort)
            );
        }
    }
}
