use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, IntoStaticStr};

/// The language a Conversation is spoken in; Chinese has a text catalog of its own and every other
/// language falls back to English (`docs/CONTEXT.md` "conversation language").
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Hash,
    Display,
    EnumIter,
    EnumString,
    IntoStaticStr,
    Serialize,
    Deserialize,
)]
#[strum(ascii_case_insensitive, serialize_all = "kebab-case")]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    #[default]
    Zh,
    En,
    EnGb,
    Ja,
    Ko,
    Es,
    Fr,
    De,
    Pt,
    Ru,
    Ar,
    It,
    Th,
}

impl Language {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    /// The code a client platform matches a client's language by: two-letter ISO 639-1. A regional
    /// variant has none of its own, so it is served by its base language — `en-gb` reads `en`.
    pub fn client_code(self) -> &'static str {
        let tag = self.as_str();
        tag.split_once('-').map_or(tag, |(base, _)| base)
    }
}

#[cfg(test)]
mod tests {
    use strum::IntoEnumIterator;

    use super::*;

    /// serde and strum read separate attributes, so the wire names are pinned here.
    #[test]
    fn wire_form_and_display_names_cover_every_variant() {
        for language in Language::iter() {
            let wire = serde_json::to_value(language).expect("serializable");
            assert_eq!(wire.as_str(), Some(language.as_str()), "{language:?}");
            assert_eq!(language.as_str().parse::<Language>().ok(), Some(language));
            assert_eq!(
                language.as_str().to_uppercase().parse::<Language>().ok(),
                Some(language),
                "{language:?}"
            );
        }
    }

    /// A client platform matches a two-letter ISO 639-1 code, so every variant carries one and a
    /// regional variant carries its base language's.
    #[test]
    fn every_client_code_is_a_two_letter_lowercase_code() {
        for language in Language::iter() {
            let code = language.client_code();
            assert_eq!(code.len(), 2, "{language:?} / {code}");
            assert!(
                code.chars().all(|c| c.is_ascii_lowercase()),
                "{language:?} / {code}"
            );
        }
        assert_eq!(Language::EnGb.client_code(), "en");
    }

    #[test]
    fn absent_configuration_speaks_chinese() {
        assert_eq!(Language::default(), Language::Zh);
    }
}
