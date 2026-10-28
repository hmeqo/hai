//! The content-policy picker's words: what it shows and the reset button.
use crate::{
    agent::{
        host_copy::HostCopy,
        prompt::text::{LanguageText, PerCatalog},
    },
    config::conversation::{ContentPolicy, Language},
};

/// The page reports the effective level and puts the fixed levels beside it.
pub(crate) fn page(language: Language, current: ContentPolicy) -> String {
    HostCopy::ContentPolicyCurrent(current).render(language)
}

pub(crate) fn reset_label(language: Language) -> &'static str {
    RESET.get(language.text_catalog())
}

const RESET: PerCatalog<&'static str> = PerCatalog {
    zh: "恢复默认",
    en: "Reset",
};
