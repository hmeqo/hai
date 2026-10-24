//! The content-policy picker's labels and the actions each button carries.

use super::copy;
use crate::{
    agent::command::dialog::DialogPage,
    config::conversation::{ContentPolicyPreset, Language},
};

/// What one button asks for. A preset is the typed spelling the command line parses too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentPolicyAction {
    Set(ContentPolicyPreset),
    Reset,
}

/// One page's buttons, in the order they are shown; the layout is the platform's.
pub(crate) fn options(language: Language) -> Vec<(String, DialogPage)> {
    ContentPolicyPreset::ALL
        .into_iter()
        .map(|preset| {
            (
                preset.as_str().to_owned(),
                DialogPage::ContentPolicy {
                    action: ContentPolicyAction::Set(preset),
                },
            )
        })
        .chain(std::iter::once((
            copy::reset_label(language).to_owned(),
            DialogPage::ContentPolicy {
                action: ContentPolicyAction::Reset,
            },
        )))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every fixed preset is a button, and reset is the one action that is not a preset.
    #[test]
    fn the_page_offers_every_preset_and_reset() {
        let actions: Vec<_> = options(Language::Zh)
            .into_iter()
            .map(|(label, page)| {
                let DialogPage::ContentPolicy { action } = page else {
                    panic!("a content-policy page carries a content-policy action");
                };
                (label, action)
            })
            .collect();

        assert_eq!(
            actions,
            [
                (
                    "safe".to_owned(),
                    ContentPolicyAction::Set(ContentPolicyPreset::Safe)
                ),
                (
                    "standard".to_owned(),
                    ContentPolicyAction::Set(ContentPolicyPreset::Standard)
                ),
                (
                    "mature".to_owned(),
                    ContentPolicyAction::Set(ContentPolicyPreset::Mature)
                ),
                (
                    "unrestricted".to_owned(),
                    ContentPolicyAction::Set(ContentPolicyPreset::Unrestricted)
                ),
                ("恢复默认".to_owned(), ContentPolicyAction::Reset),
            ]
        );
    }
}
