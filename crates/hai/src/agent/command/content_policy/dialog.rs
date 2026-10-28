//! The content-policy picker's labels and the actions each button carries.

use super::copy;
use crate::{
    agent::{command::dialog::DialogPage, host_copy::level_label},
    config::conversation::{ContentPolicy, Language},
};

/// What one button asks for. A level is the typed spelling the command line parses too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentPolicyAction {
    Set(ContentPolicy),
    Reset,
}

/// One page's buttons, in the order they are shown; the layout is the platform's.
pub(crate) fn options(language: Language) -> Vec<(String, DialogPage)> {
    ContentPolicy::ALL
        .into_iter()
        .map(|level| {
            (
                level_label(language, level).to_owned(),
                DialogPage::ContentPolicy {
                    action: ContentPolicyAction::Set(level),
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

    /// Every level is a button, and reset is the one action that is not a level.
    #[test]
    fn the_page_offers_every_level_and_reset() {
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
                    "安全".to_owned(),
                    ContentPolicyAction::Set(ContentPolicy::Safe)
                ),
                (
                    "标准".to_owned(),
                    ContentPolicyAction::Set(ContentPolicy::Standard)
                ),
                (
                    "无限制".to_owned(),
                    ContentPolicyAction::Set(ContentPolicy::Unrestricted)
                ),
                ("恢复默认".to_owned(), ContentPolicyAction::Reset),
            ]
        );

        let labels: Vec<_> = options(Language::En)
            .into_iter()
            .map(|(label, _)| label)
            .collect();
        assert_eq!(labels, ["safe", "standard", "unrestricted", "Reset"]);
    }
}
