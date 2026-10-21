//! The dialog engine: a page of buttons put out on a channel, the presses read back from it, and the
//! state each button carries. A command that asks before it acts draws its pages through this; what a
//! press means is settled by the command that drew the page.

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};

use uuid::Uuid;

use super::{
    digest::{DigestAction, DigestDialog},
    purge::{PurgeAction, PurgeDialog},
};
use crate::{
    agent::channel::{CallbackAnswer, ChannelAccess, ChoiceOption},
    agentcore::Language,
    domain::{model::ChatType, vo::ChatId},
    error::Result,
};

/// The button one token belongs to: which command drew the page, that command's own state, and the
/// button that was pressed. The engine never reads past this enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DialogPage {
    Purge {
        state: PurgeDialog,
        action: PurgeAction,
    },
    Digest {
        state: DigestDialog,
        action: DigestAction,
    },
}

/// Why a press could not be read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Miss {
    /// The page is gone: never registered, already pressed, or too old.
    Stale,
    /// The press came from another chat, or from somebody the page does not admit.
    Foreign,
}

/// The chat a page speaks to: its id, the platform's id for it, the kind that picks the labels, and
/// the language it is written in. Everything that draws or rewrites a page reads the chat from here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageSite<'a> {
    pub chat_id: ChatId,
    pub chat_external_id: &'a str,
    pub chat_type: ChatType,
    pub language: Language,
}

/// One button press: the chat the page speaks to, the message its buttons hang on, the token and the
/// callback the button carried, and who pressed.
pub(crate) struct Press<'a> {
    pub site: PageSite<'a>,
    pub message_external_id: &'a str,
    pub token: &'a str,
    pub query_id: &'a str,
    pub sender_id: Option<&'a str>,
}

/// How long a page's buttons stay usable.
pub(crate) const CHOICE_TTL: Duration = Duration::from_secs(1800);

/// How many buttons may wait at once; the oldest are dropped as time passes, this bounds the rest.
pub(crate) const MAX_PENDING_CHOICES: usize = 256;

/// Who may press a page's buttons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Audience {
    /// The person who opened the dialog. `None` when the platform reported nobody, which only a
    /// press that names nobody may use.
    Opener(Option<String>),
    /// Anybody in the chat: a page that acts on nobody in particular.
    Chat,
}

/// The pages waiting for a press. Held in memory and bound to the chat: a dialog is an immediate
/// exchange, and nothing about it is worth a table.
#[derive(Default)]
pub(crate) struct Pages {
    entries: Mutex<HashMap<String, Entry>>,
}

#[derive(Debug, Clone)]
struct Entry {
    chat_id: ChatId,
    audience: Audience,
    page: DialogPage,
    at: Instant,
}

impl Pages {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// One token per button, all bound to this chat and this audience; registration stops at the cap,
    /// so a page drawn at capacity carries fewer buttons rather than dropping somebody else's dialog.
    pub(crate) fn draw(
        &self,
        site: PageSite<'_>,
        audience: Audience,
        buttons: &[(String, DialogPage)],
    ) -> Vec<ChoiceOption> {
        let mut entries = self.lock();
        let now = Instant::now();
        entries.retain(|_, entry| now.duration_since(entry.at) < CHOICE_TTL);

        let mut options = Vec::with_capacity(buttons.len());
        for (label, page) in buttons {
            if entries.len() >= MAX_PENDING_CHOICES {
                tracing::warn!(
                    chat_id = %site.chat_id,
                    "dialog at capacity; a button was left out",
                );
                break;
            }
            let token = Uuid::now_v7().to_string();
            entries.insert(
                token.clone(),
                Entry {
                    chat_id: site.chat_id,
                    audience: audience.clone(),
                    page: page.clone(),
                    at: now,
                },
            );
            options.push(ChoiceOption {
                label: label.clone(),
                token,
            });
        }
        options
    }

    /// Reads one press back and spends it: a token is good for exactly one press. A page that is gone
    /// is stale, and a press from somebody the page does not admit is refused without being spent —
    /// the person who opened the dialog keeps it.
    pub(crate) fn take(&self, press: &Press<'_>) -> Result<DialogPage, Miss> {
        let mut entries = self.lock();
        let now = Instant::now();
        entries.retain(|_, entry| now.duration_since(entry.at) < CHOICE_TTL);

        let Some(entry) = entries.get(press.token).cloned() else {
            return Err(Miss::Stale);
        };
        if entry.chat_id != press.site.chat_id {
            return Err(Miss::Foreign);
        }
        if let Audience::Opener(opener) = &entry.audience
            && opener.as_deref() != press.sender_id
        {
            return Err(Miss::Foreign);
        }
        entries.remove(press.token);
        Ok(entry.page)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Draws the next page where the pressed one stands and answers the press. The rewrite is best
/// effort — a platform that refuses it gets the same page as a fresh message — and the press is
/// answered either way: a button that does nothing leaves the client waiting.
pub(crate) async fn settle(
    access: &dyn ChannelAccess,
    site: &PageSite<'_>,
    message_external_id: &str,
    text: &str,
    options: &[ChoiceOption],
    query_id: &str,
) -> Result<()> {
    if let Err(e) = rewrite_or_ask(access, site, message_external_id, text, options).await {
        tracing::warn!(chat_id = %site.chat_id, error = %e, "a dialog page could not be delivered");
    }
    access
        .answer_callback(query_id, CallbackAnswer::Quiet)
        .await
}

/// Rewrites the page where it is; when the platform refuses, the same text and buttons go out as a
/// fresh message — a page nobody can press again is worse than a second message.
pub(crate) async fn rewrite_or_ask(
    access: &dyn ChannelAccess,
    site: &PageSite<'_>,
    message_external_id: &str,
    text: &str,
    options: &[ChoiceOption],
) -> Result<()> {
    match access
        .edit_choice(site.chat_external_id, message_external_id, text, options)
        .await
    {
        Ok(()) => Ok(()),
        Err(e) => {
            tracing::warn!(error = %e, "a dialog page could not be rewritten; a fresh one goes out");
            access.ask(site.chat_external_id, text, options).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::vo::PurgeSurfaces;

    const CHAT: ChatId = ChatId(7);
    const SENDER: Option<&str> = Some("42");

    /// The chat every test page speaks to.
    fn site() -> PageSite<'static> {
        PageSite {
            chat_id: CHAT,
            chat_external_id: "7",
            chat_type: ChatType::Private,
            language: Language::Zh,
        }
    }

    /// A press in `CHAT` from `SENDER`, with the callback and the message the buttons hang on.
    fn press(token: &str) -> Press<'_> {
        Press {
            site: site(),
            message_external_id: "m",
            token,
            query_id: "q",
            sender_id: SENDER,
        }
    }

    /// A page a purge drew: pressable only by the person who opened it.
    fn owner() -> Audience {
        Audience::Opener(SENDER.map(str::to_owned))
    }

    fn page(action: PurgeAction) -> DialogPage {
        DialogPage::Purge {
            state: PurgeDialog::Stretch {
                surfaces: PurgeSurfaces::all(),
            },
            action,
        }
    }

    fn digest_page(action: DigestAction) -> DialogPage {
        DialogPage::Digest {
            state: DigestDialog::Stretch {
                language: None,
                delivery: None,
            },
            action,
        }
    }

    /// Two commands' pages wait side by side, and a token stays with the page it was drawn for.
    #[test]
    fn two_commands_pages_do_not_cross() {
        let pages = Pages::new();
        let purge = pages.draw(
            site(),
            owner(),
            &[("Cancel".to_owned(), page(PurgeAction::Cancel))],
        );
        let digest = pages.draw(
            site(),
            owner(),
            &[("Close".to_owned(), digest_page(DigestAction::Close))],
        );

        assert!(matches!(
            pages.take(&press(&purge[0].token)),
            Ok(DialogPage::Purge { .. })
        ));
        assert!(matches!(
            pages.take(&press(&digest[0].token)),
            Ok(DialogPage::Digest { .. })
        ));
        assert_eq!(
            pages.take(&press(&digest[0].token)).err(),
            Some(Miss::Stale),
            "a token is still good for exactly one press"
        );
    }

    /// One token is one press: a button that was already used cannot be used again.
    #[test]
    fn a_token_is_good_for_exactly_one_press() {
        let pages = Pages::new();
        let drawn = pages.draw(
            site(),
            owner(),
            &[("Cancel".to_owned(), page(PurgeAction::Cancel))],
        );
        assert_eq!(drawn.len(), 1);
        let token = &drawn[0].token;

        let taken = pages.take(&press(token)).expect("the first press");
        assert_eq!(taken, page(PurgeAction::Cancel));
        assert_eq!(pages.take(&press(token)).err(), Some(Miss::Stale));
    }

    /// A page left overnight is not a confirmation any more.
    #[test]
    fn an_expired_page_is_stale() {
        let pages = Pages::new();
        {
            let mut entries = pages.lock();
            entries.insert(
                "old".to_owned(),
                Entry {
                    chat_id: CHAT,
                    audience: owner(),
                    page: page(PurgeAction::Back),
                    at: Instant::now() - CHOICE_TTL - Duration::from_secs(1),
                },
            );
        }

        assert_eq!(pages.take(&press("old")).err(), Some(Miss::Stale));
    }

    /// Somebody else's press is refused and spends nothing: the dialog stays with the person who
    /// opened it.
    #[test]
    fn a_foreign_press_is_refused_without_spending_the_token() {
        let pages = Pages::new();
        let token = pages
            .draw(
                site(),
                owner(),
                &[("Cancel".to_owned(), page(PurgeAction::Cancel))],
            )
            .remove(0)
            .token;

        let mut from_another = press(&token);
        from_another.sender_id = Some("43");
        assert_eq!(pages.take(&from_another).err(), Some(Miss::Foreign));

        let mut in_another_chat = press(&token);
        in_another_chat.site.chat_id = ChatId(8);
        assert_eq!(
            pages.take(&in_another_chat).err(),
            Some(Miss::Foreign),
            "another chat's token is not this chat's"
        );
        assert!(
            pages.take(&press(&token)).is_ok(),
            "the owner can still use it"
        );
    }

    /// A page nobody owns admits anybody in the chat: its buttons are the chat's, not the opener's.
    #[test]
    fn a_chat_page_admits_anybody() {
        let pages = Pages::new();
        let token = pages
            .draw(
                site(),
                Audience::Chat,
                &[("Close".to_owned(), digest_page(DigestAction::Close))],
            )
            .remove(0)
            .token;

        let mut from_another = press(&token);
        from_another.sender_id = Some("43");
        assert_eq!(
            pages
                .take(&from_another)
                .expect("the page admits anybody in the chat"),
            digest_page(DigestAction::Close)
        );
    }

    /// A rewrite the platform accepts needs no second message.
    #[tokio::test]
    async fn a_rewrite_that_works_asks_nothing() {
        let access = crate::agent::test_support::RecordingAccess::default();

        rewrite_or_ask(&access, &site(), "m", "the page", &[])
            .await
            .expect("the page is rewritten");

        assert_eq!(access.edited.lock().expect("test channel lock").len(), 1);
        assert!(
            access.asked.lock().expect("test channel lock").is_empty(),
            "no second message is sent"
        );
    }

    /// A refused rewrite sends the same page as a fresh message, so the buttons stay pressable.
    #[tokio::test]
    async fn a_failed_rewrite_sends_a_fresh_page() {
        let access = crate::agent::test_support::RecordingAccess {
            edit_fails: true,
            ..Default::default()
        };

        rewrite_or_ask(&access, &site(), "m", "the page", &[])
            .await
            .expect("the fresh page goes out");

        let asked = access.asked.lock().expect("test channel lock");
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].prompt, "the page");
        assert_eq!(asked[0].chat_external_id, "7");
    }

    #[test]
    fn a_full_table_hands_out_no_more_tokens() {
        let pages = Pages::new();
        let mut registered = 0;
        while pages.lock().len() < MAX_PENDING_CHOICES {
            let drawn = pages.draw(
                site(),
                owner(),
                &[("Cancel".to_owned(), page(PurgeAction::Cancel))],
            );
            assert_eq!(drawn.len(), 1);
            registered += 1;
        }

        let extra = pages.draw(
            site(),
            owner(),
            &[("Cancel".to_owned(), page(PurgeAction::Cancel))],
        );
        assert!(extra.is_empty(), "nothing is registered past the cap");
        assert_eq!(pages.lock().len(), MAX_PENDING_CHOICES);
        assert!(registered > 0);
    }
}
