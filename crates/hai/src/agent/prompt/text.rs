//! Text catalogs: which built-in language a prompt or host-copy section reads.

use strum::EnumIter;

use crate::config::conversation::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter)]
pub(crate) enum TextCatalog {
    Zh,
    En,
}

/// A value that has one form per text catalog. Every literal names both fields, so a new catalog
/// stops compiling here and at every construction site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PerCatalog<T> {
    pub zh: T,
    pub en: T,
}

impl<T> PerCatalog<T> {
    pub fn get(self, catalog: TextCatalog) -> T {
        match catalog {
            TextCatalog::Zh => self.zh,
            TextCatalog::En => self.en,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LanguageName(&'static str);

impl LanguageName {
    pub(crate) const fn new(name: &'static str) -> Self {
        Self(name)
    }

    pub(crate) fn as_str(self) -> &'static str {
        self.0
    }
}

pub(crate) trait LanguageText {
    fn text_catalog(self) -> TextCatalog;
    fn names(self) -> PerCatalog<LanguageName>;
}

impl LanguageText for Language {
    fn text_catalog(self) -> TextCatalog {
        match self {
            Self::Zh => TextCatalog::Zh,
            _ => TextCatalog::En,
        }
    }

    fn names(self) -> PerCatalog<LanguageName> {
        match self {
            Self::Zh => PerCatalog {
                zh: LanguageName::new("中文"),
                en: LanguageName::new("Chinese"),
            },
            Self::En => PerCatalog {
                zh: LanguageName::new("英语"),
                en: LanguageName::new("English"),
            },
            Self::EnGb => PerCatalog {
                zh: LanguageName::new("英式英语"),
                en: LanguageName::new("British English"),
            },
            Self::Ja => PerCatalog {
                zh: LanguageName::new("日语"),
                en: LanguageName::new("Japanese"),
            },
            Self::Ko => PerCatalog {
                zh: LanguageName::new("韩语"),
                en: LanguageName::new("Korean"),
            },
            Self::Es => PerCatalog {
                zh: LanguageName::new("西班牙语"),
                en: LanguageName::new("Spanish"),
            },
            Self::Fr => PerCatalog {
                zh: LanguageName::new("法语"),
                en: LanguageName::new("French"),
            },
            Self::De => PerCatalog {
                zh: LanguageName::new("德语"),
                en: LanguageName::new("German"),
            },
            Self::Pt => PerCatalog {
                zh: LanguageName::new("葡萄牙语"),
                en: LanguageName::new("Portuguese"),
            },
            Self::Ru => PerCatalog {
                zh: LanguageName::new("俄语"),
                en: LanguageName::new("Russian"),
            },
            Self::Ar => PerCatalog {
                zh: LanguageName::new("阿拉伯语"),
                en: LanguageName::new("Arabic"),
            },
            Self::It => PerCatalog {
                zh: LanguageName::new("意大利语"),
                en: LanguageName::new("Italian"),
            },
            Self::Th => PerCatalog {
                zh: LanguageName::new("泰语"),
                en: LanguageName::new("Thai"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use strum::IntoEnumIterator;

    use super::*;

    #[test]
    fn get_picks_the_field_matching_the_catalog() {
        let value = PerCatalog {
            zh: "中",
            en: "English",
        };

        assert_eq!(value.get(TextCatalog::Zh), "中");
        assert_eq!(value.get(TextCatalog::En), "English");
    }

    #[test]
    fn every_language_has_a_name_in_every_catalog() {
        for language in Language::iter() {
            for catalog in TextCatalog::iter() {
                assert!(
                    !language.names().get(catalog).as_str().is_empty(),
                    "{language:?} / {catalog:?}"
                );
            }
        }
    }

    /// Every catalog is one some language reads: text in a catalog nothing maps to is unreachable,
    /// so a new catalog has to route a language to it.
    #[test]
    fn every_catalog_is_read_by_some_language() {
        for catalog in TextCatalog::iter() {
            assert!(
                Language::iter().any(|language| language.text_catalog() == catalog),
                "{catalog:?}"
            );
        }
    }
}
