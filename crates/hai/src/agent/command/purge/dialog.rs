//! The purge confirmation's pages and the buttons each one offers.

use crate::{
    agentcore::Language,
    domain::vo::{PurgeSurface, PurgeSurfaces, Stretch},
};

/// Where a confirmation stands. Each page carries what it needs, so the page that names a stretch
/// always has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PurgeDialog {
    /// The first page: the two questions to choose between, and the surfaces chosen so far beside
    /// them.
    Menu { surfaces: PurgeSurfaces },
    /// Choosing the stretch.
    Stretch { surfaces: PurgeSurfaces },
    /// Toggling what the purge holds.
    Surfaces { surfaces: PurgeSurfaces },
    /// Ready to run.
    Confirm {
        range: Stretch,
        surfaces: PurgeSurfaces,
    },
}

impl PurgeDialog {
    pub(crate) fn surfaces(&self) -> PurgeSurfaces {
        match *self {
            Self::Menu { surfaces }
            | Self::Stretch { surfaces }
            | Self::Surfaces { surfaces }
            | Self::Confirm { surfaces, .. } => surfaces,
        }
    }

    /// The same page with one surface flipped; a page never loses what it was told.
    pub(crate) fn toggled(&self, surface: PurgeSurface) -> Self {
        let surfaces = self.surfaces().toggled(surface);
        match *self {
            Self::Menu { .. } => Self::Menu { surfaces },
            Self::Stretch { .. } => Self::Stretch { surfaces },
            Self::Surfaces { .. } => Self::Surfaces { surfaces },
            Self::Confirm { range, .. } => Self::Confirm { range, surfaces },
        }
    }

    /// The confirmation page for `range`, keeping the surfaces chosen so far.
    pub(crate) fn confirm(&self, range: Stretch) -> Self {
        Self::Confirm {
            range,
            surfaces: self.surfaces(),
        }
    }
}

/// What one button asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PurgeAction {
    /// On to the stretch page.
    Stretch,
    /// Back to the first page.
    Back,
    /// On to the surfaces page (`What to purge…`).
    Surfaces,
    Toggle(PurgeSurface),
    /// Take this stretch and ask for the confirmation.
    Take(Stretch),
    Confirm,
    Cancel,
}

/// One page's buttons, in the order they are shown; the layout is the platform's.
pub(crate) fn options(language: Language, dialog: &PurgeDialog) -> Vec<(String, PurgeAction)> {
    let labels = super::copy::labels(language);
    match *dialog {
        PurgeDialog::Menu { .. } => vec![
            (labels.range.to_owned(), PurgeAction::Stretch),
            (labels.content.to_owned(), PurgeAction::Surfaces),
            (labels.cancel.to_owned(), PurgeAction::Cancel),
        ],
        PurgeDialog::Stretch { .. } => super::copy::stretches(language)
            .into_iter()
            .map(|(stretch, label)| (label, PurgeAction::Take(stretch)))
            .chain([
                (labels.back.to_owned(), PurgeAction::Back),
                (labels.cancel.to_owned(), PurgeAction::Cancel),
            ])
            .collect(),
        PurgeDialog::Surfaces { .. } => super::copy::surfaces(language)
            .into_iter()
            .map(|(surface, label)| (label.to_owned(), PurgeAction::Toggle(surface)))
            .chain([
                (labels.back.to_owned(), PurgeAction::Back),
                (labels.cancel.to_owned(), PurgeAction::Cancel),
            ])
            .collect(),
        PurgeDialog::Confirm { .. } => vec![
            (labels.confirm.to_owned(), PurgeAction::Confirm),
            (labels.cancel.to_owned(), PurgeAction::Cancel),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dialog_toggles_one_surface_without_losing_its_page() {
        let dialog = PurgeDialog::Confirm {
            range: Stretch::All,
            surfaces: PurgeSurfaces::all(),
        };

        match dialog.toggled(PurgeSurface::Telegram) {
            PurgeDialog::Confirm { range, surfaces } => {
                assert_eq!(range, Stretch::All);
                assert!(!surfaces.telegram);
                assert!(surfaces.agent);
            }
            other => panic!("expected the confirmation page; got {other:?}"),
        }
    }

    /// The surfaces chosen before the stretch are the ones the confirmation names.
    #[test]
    fn the_confirmation_keeps_the_surfaces_chosen_so_far() {
        let chosen = PurgeSurfaces::none().with(PurgeSurface::Agent);
        let confirmed = PurgeDialog::Stretch { surfaces: chosen }.confirm(Stretch::All);
        let PurgeDialog::Confirm { surfaces, .. } = confirmed else {
            panic!("expected the confirmation page");
        };
        assert!(surfaces.agent && !surfaces.files);
    }

    /// Every page's buttons are the ones it can act on, in the order they are shown.
    #[test]
    fn each_page_offers_its_own_buttons() {
        let actions = |dialog| {
            options(Language::Zh, &dialog)
                .into_iter()
                .map(|(_, action)| action)
                .collect::<Vec<_>>()
        };
        let surfaces = PurgeSurfaces::all();

        assert_eq!(
            actions(PurgeDialog::Menu { surfaces }),
            vec![
                PurgeAction::Stretch,
                PurgeAction::Surfaces,
                PurgeAction::Cancel
            ],
            "the first page offers the two questions and a way out"
        );

        let stretches = actions(PurgeDialog::Stretch { surfaces });
        assert_eq!(stretches.len(), 9);
        assert_eq!(
            stretches[0],
            PurgeAction::Take(Stretch::Last { secs: 300 }),
            "the closest stretch offered is the last five minutes"
        );
        assert_eq!(
            stretches[1],
            PurgeAction::Take(Stretch::Last { secs: 1_800 })
        );
        assert_eq!(
            stretches[2],
            PurgeAction::Take(Stretch::Last { secs: 3_600 })
        );
        assert_eq!(
            stretches[6],
            PurgeAction::Take(Stretch::All),
            "the last stretch offered is all time"
        );
        assert_eq!(stretches[7], PurgeAction::Back);
        assert_eq!(stretches[8], PurgeAction::Cancel);

        let toggles = actions(PurgeDialog::Surfaces { surfaces });
        assert_eq!(toggles.len(), 6);
        assert_eq!(toggles[0], PurgeAction::Toggle(PurgeSurface::Agent));
        assert_eq!(toggles[3], PurgeAction::Toggle(PurgeSurface::Derived));
        assert_eq!(toggles[4], PurgeAction::Back);
        assert_eq!(toggles[5], PurgeAction::Cancel);

        assert_eq!(
            actions(PurgeDialog::Confirm {
                range: Stretch::All,
                surfaces,
            }),
            vec![PurgeAction::Confirm, PurgeAction::Cancel]
        );
    }

    /// The first page names both questions it offers and shows what the purge would hold.
    #[test]
    fn the_first_page_names_both_questions() {
        let text = super::super::copy::stage_text(
            Language::Zh,
            &PurgeDialog::Menu {
                surfaces: PurgeSurfaces::all(),
            },
        );

        assert!(text.contains("时间范围"), "{text}");
        assert!(text.contains("清理内容"), "{text}");
        assert!(
            text.contains("agent 痕迹"),
            "the surfaces are listed: {text}"
        );
    }

    /// The range page offers a span per button, the five-minute window first and all time last;
    /// the labels are the names the page itself uses.
    #[test]
    fn the_stretch_page_offers_a_span_per_button() {
        for language in [Language::Zh, Language::En] {
            let offered = super::super::copy::stretches(language);
            assert_eq!(offered[0].0, Stretch::Last { secs: 300 });
            assert_eq!(offered[6].0, Stretch::All);
        }
    }
}
