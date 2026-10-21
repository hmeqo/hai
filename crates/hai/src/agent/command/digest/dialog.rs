//! The digest dialog's pages and the buttons each one offers.

use std::sync::Arc;

use super::copy;
use crate::{
    agentcore::Language,
    domain::{model::ChatType, vo::Stretch},
};

/// Where a finished report goes: the chat the dialog lives in, or the person who pressed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Delivery {
    Chat,
    Person,
}

/// Where a digest stands. Each page carries what it needs, so the pages that name a report have one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DigestDialog {
    /// Choosing the stretch.
    Stretch { language: Option<Language> },
    /// About to run.
    Confirm {
        stretch: Stretch,
        language: Option<Language>,
    },
    /// The pass is running, and where its report will land once it is written.
    Running {
        stretch: Stretch,
        delivery: Delivery,
    },
    /// The pass wrote a report but the platform refused the delivery; the report waits for a retry.
    Undelivered {
        stretch: Stretch,
        report: Arc<str>,
        truncated: bool,
        target: Delivery,
        error: Arc<str>,
    },
    /// The pass failed, in its own words; a retry keeps the destination it was asked for.
    Failed {
        stretch: Stretch,
        error: Arc<str>,
        language: Option<Language>,
        delivery: Delivery,
    },
}

impl DigestDialog {
    /// The report language this page was asked for, when it was named.
    pub(crate) fn language(&self) -> Option<Language> {
        match *self {
            Self::Stretch { language }
            | Self::Confirm { language, .. }
            | Self::Failed { language, .. } => language,
            Self::Running { .. } | Self::Undelivered { .. } => None,
        }
    }
}

/// What one button asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DigestAction {
    /// Back to the stretch page.
    Back,
    /// The advice that another stretch is typed as `/digest <days>`.
    Custom,
    /// Take this stretch and ask for the confirmation.
    Take(Stretch),
    /// Run the pass and deliver its report to this destination.
    Start(Delivery),
    /// Deliver the retained report, which a refused delivery left on the page, to this destination.
    Deliver(Delivery),
    /// Run the pass again after a failed pass.
    Retry,
    Close,
}

/// One page's buttons, in the order they are shown; the layout is the platform's. The chat's kind
/// decides what the delivery button promises.
pub(crate) fn options(
    language: Language,
    chat_type: ChatType,
    dialog: &DigestDialog,
) -> Vec<(String, DigestAction)> {
    let labels = copy::labels(language);
    let private = matches!(chat_type, ChatType::Private);
    match *dialog {
        DigestDialog::Stretch { .. } => copy::presets(language)
            .into_iter()
            .map(|(stretch, label)| (label, DigestAction::Take(stretch)))
            .chain([
                (labels.custom.to_owned(), DigestAction::Custom),
                (labels.close.to_owned(), DigestAction::Close),
            ])
            .collect(),
        DigestDialog::Confirm { .. } if private => vec![
            (labels.start.to_owned(), DigestAction::Start(Delivery::Chat)),
            (labels.back.to_owned(), DigestAction::Back),
            (labels.close.to_owned(), DigestAction::Close),
        ],
        DigestDialog::Confirm { .. } => vec![
            (labels.dm.to_owned(), DigestAction::Start(Delivery::Person)),
            (labels.here.to_owned(), DigestAction::Start(Delivery::Chat)),
            (labels.back.to_owned(), DigestAction::Back),
            (labels.close.to_owned(), DigestAction::Close),
        ],
        DigestDialog::Running { .. } => Vec::new(),
        DigestDialog::Undelivered {
            target: Delivery::Person,
            ..
        } => vec![
            (
                labels.retry.to_owned(),
                DigestAction::Deliver(Delivery::Person),
            ),
            (
                labels.here.to_owned(),
                DigestAction::Deliver(Delivery::Chat),
            ),
            (labels.close.to_owned(), DigestAction::Close),
        ],
        DigestDialog::Undelivered { .. } => vec![
            (
                labels.retry.to_owned(),
                DigestAction::Deliver(Delivery::Chat),
            ),
            (labels.close.to_owned(), DigestAction::Close),
        ],
        DigestDialog::Failed { .. } => vec![
            (labels.retry.to_owned(), DigestAction::Retry),
            (labels.close.to_owned(), DigestAction::Close),
        ],
    }
}

/// The advice page's own buttons: back to the presets, or out.
pub(crate) fn hint_options(language: Language) -> Vec<(String, DigestAction)> {
    let labels = copy::labels(language);
    vec![
        (labels.back.to_owned(), DigestAction::Back),
        (labels.close.to_owned(), DigestAction::Close),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every page offers the buttons it can act on, in the order they are shown.
    #[test]
    fn each_page_offers_its_own_buttons() {
        let actions = |dialog| {
            options(Language::Zh, ChatType::Group, &dialog)
                .into_iter()
                .map(|(_, action)| action)
                .collect::<Vec<_>>()
        };
        let week = Stretch::Last { secs: 7 * 86_400 };

        assert_eq!(
            actions(DigestDialog::Stretch { language: None }),
            vec![
                DigestAction::Take(Stretch::Last { secs: 86_400 }),
                DigestAction::Take(week),
                DigestAction::Take(Stretch::Last { secs: 30 * 86_400 }),
                DigestAction::Custom,
                DigestAction::Close,
            ]
        );
        assert_eq!(
            actions(DigestDialog::Confirm {
                stretch: week,
                language: None,
            }),
            vec![
                DigestAction::Start(Delivery::Person),
                DigestAction::Start(Delivery::Chat),
                DigestAction::Back,
                DigestAction::Close,
            ]
        );
        assert!(
            actions(DigestDialog::Running {
                stretch: week,
                delivery: Delivery::Chat,
            })
            .is_empty(),
            "a running page offers nothing to press"
        );
        assert_eq!(
            actions(DigestDialog::Undelivered {
                stretch: week,
                report: "report".into(),
                truncated: false,
                target: Delivery::Person,
                error: "boom".into(),
            }),
            vec![
                DigestAction::Deliver(Delivery::Person),
                DigestAction::Deliver(Delivery::Chat),
                DigestAction::Close
            ]
        );
        assert_eq!(
            actions(DigestDialog::Undelivered {
                stretch: week,
                report: "report".into(),
                truncated: false,
                target: Delivery::Chat,
                error: "boom".into(),
            }),
            vec![DigestAction::Deliver(Delivery::Chat), DigestAction::Close]
        );
        assert_eq!(
            actions(DigestDialog::Failed {
                stretch: week,
                error: "boom".into(),
                language: None,
                delivery: Delivery::Chat,
            }),
            vec![DigestAction::Retry, DigestAction::Close]
        );
    }

    /// A private chat has one destination; a group asks which one.
    #[test]
    fn a_private_chat_offers_only_the_chat() {
        let week = Stretch::Last { secs: 86_400 };
        let confirm = DigestDialog::Confirm {
            stretch: week,
            language: None,
        };
        let buttons = |chat_type| {
            options(Language::Zh, chat_type, &confirm)
                .into_iter()
                .collect::<Vec<_>>()
        };

        assert_eq!(
            buttons(ChatType::Private),
            vec![
                ("生成报告".to_owned(), DigestAction::Start(Delivery::Chat)),
                ("返回".to_owned(), DigestAction::Back),
                ("关闭".to_owned(), DigestAction::Close),
            ]
        );
        let group = buttons(ChatType::Group);
        assert_eq!(
            group[0],
            ("私信给我".to_owned(), DigestAction::Start(Delivery::Person))
        );
        assert_eq!(
            group[1],
            ("发到本群".to_owned(), DigestAction::Start(Delivery::Chat))
        );
    }

    /// The advice page is a way back and a way out, nothing else.
    #[test]
    fn the_advice_page_only_goes_back_or_out() {
        assert_eq!(
            hint_options(Language::En)
                .into_iter()
                .map(|(_, action)| action)
                .collect::<Vec<_>>(),
            vec![DigestAction::Back, DigestAction::Close]
        );
    }
}
