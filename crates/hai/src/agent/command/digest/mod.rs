//! `/digest`: a report over a stretch of the chat's own record. The dialog is host copy in memory,
//! the pass runs in the chat's own Busy slot, and neither becomes chat data — no entry, no message
//! row; the pass's own `digest-*` and step events are its audit trail.

pub(crate) mod copy;
mod dialog;
mod prompt;
pub(crate) mod run;

use std::sync::Arc;

pub(crate) use dialog::{Delivery, DigestAction, DigestDialog};
pub(crate) use run::{DigestPass, run};

use crate::{
    agent::{
        channel::{CallbackAnswer, ChannelAccess, ChoiceOption},
        command::{
            CommandContext, DigestArg, MAX_DIGEST_DAYS, MIN_DIGEST_DAYS,
            dialog::{Audience, DialogPage, PageSite, Pages, Press, rewrite_or_ask, settle},
        },
        host_copy::HostCopy,
        inbound::Arrival,
    },
    agentcore::Language,
    domain::vo::{ChatId, Stretch},
    error::Result,
};

/// Puts the first page out. A command that already named a stretch goes straight to the confirmation;
/// a bare `/digest` starts by choosing one.
pub(crate) async fn open(
    pages: &Pages,
    access: &dyn ChannelAccess,
    chat_id: ChatId,
    arrival: &Arrival,
    arg: DigestArg,
    language: Language,
) -> Result<()> {
    let state = match arg.stretch {
        Some(stretch) => DigestDialog::Confirm {
            stretch,
            language: arg.language,
        },
        None => DigestDialog::Stretch {
            language: arg.language,
        },
    };
    let site = PageSite {
        chat_id,
        chat_external_id: &arrival.chat.external_id,
        chat_type: arrival.chat.kind,
        language,
    };
    let options = page(pages, site, &state);
    access
        .ask(
            site.chat_external_id,
            &copy::stage_text(language, &state),
            &options,
        )
        .await
}

/// One button press on a digest page. The page table is the shared one, so the handles arrive owned:
/// a started pass outlives this call.
pub(crate) async fn apply(
    pages: Arc<Pages>,
    access: Arc<dyn ChannelAccess>,
    ctx: &CommandContext,
    state: DigestDialog,
    action: DigestAction,
    press: &Press<'_>,
) -> Result<()> {
    let language = press.site.language;
    let report_language = state.language();

    // Pages that only turn to another one settle their text here; the arms that start work or
    // deliver return early with a page of their own.
    let (state, text, actions) = match action {
        DigestAction::Close => (state, copy::closed(language), Vec::new()),
        DigestAction::Custom => (
            state,
            copy::custom_hint(language, MIN_DIGEST_DAYS, MAX_DIGEST_DAYS),
            dialog::hint_options(language),
        ),
        DigestAction::Back => {
            let state = DigestDialog::Stretch {
                language: report_language,
            };
            let text = copy::stage_text(language, &state);
            let actions = dialog::options(language, press.site.chat_type, &state);
            (state, text, actions)
        }
        DigestAction::Take(stretch) => {
            let state = DigestDialog::Confirm {
                stretch,
                language: report_language,
            };
            let text = copy::stage_text(language, &state);
            let actions = dialog::options(language, press.site.chat_type, &state);
            (state, text, actions)
        }
        DigestAction::Start(delivery) => {
            let DigestDialog::Confirm { stretch, .. } = &state else {
                return stale(&*access, press).await;
            };
            let Some(chat) = destination(delivery, press) else {
                return refuse(&pages, &*access, press, &state).await;
            };
            let request = Request {
                stretch: *stretch,
                report_language,
                delivery,
                chat,
            };
            return start(pages, access, ctx, request, press).await;
        }
        DigestAction::Retry => {
            let DigestDialog::Failed {
                stretch, delivery, ..
            } = &state
            else {
                return stale(&*access, press).await;
            };
            let (stretch, delivery) = (*stretch, *delivery);
            let Some(chat) = destination(delivery, press) else {
                return refuse(&pages, &*access, press, &state).await;
            };
            let request = Request {
                stretch,
                report_language,
                delivery,
                chat,
            };
            return start(pages, access, ctx, request, press).await;
        }
        // `redeliver` refuses a state that is not the undelivered one.
        DigestAction::Deliver(delivery) => {
            return redeliver(&pages, &*access, state, delivery, press).await;
        }
    };

    let options = draw(&pages, press.site, &state, actions);
    settle(
        &*access,
        &press.site,
        press.message_external_id,
        &text,
        &options,
        press.query_id,
    )
    .await
}

/// The platform chat a delivery goes to for this press, or `None` when a person-targeted press named
/// nobody the platform could deliver to.
fn destination<'a>(delivery: Delivery, press: &Press<'a>) -> Option<&'a str> {
    match delivery {
        Delivery::Chat => Some(press.site.chat_external_id),
        Delivery::Person => press.sender_id,
    }
}

/// One pass as a page asked for it: the window it reads, the language of its report, and where the
/// report lands.
struct Request<'a> {
    stretch: Stretch,
    report_language: Option<Language>,
    delivery: Delivery,
    chat: &'a str,
}

/// Starts the pass and turns the page to "running". The completion delivers the report to the
/// destination the press named. The rewrite is best effort: the pass is already accepted, so a page
/// the platform refuses to repaint is only logged.
async fn start(
    pages: Arc<Pages>,
    access: Arc<dyn ChannelAccess>,
    ctx: &CommandContext,
    request: Request<'_>,
    press: &Press<'_>,
) -> Result<()> {
    let Request {
        stretch,
        report_language,
        delivery,
        chat,
    } = request;
    let site = press.site;
    let language = site.language;
    let running = DigestDialog::Running { stretch, delivery };
    match ctx
        .agents
        .get_or_create(ctx.chat_id)
        .await
        .request_digest(stretch, report_language)
    {
        Ok(reply) => {
            if let Err(e) = access
                .edit_choice(
                    site.chat_external_id,
                    press.message_external_id,
                    &copy::stage_text(language, &running),
                    &[],
                )
                .await
            {
                tracing::warn!(chat_id = %site.chat_id, error = %e, "the running page could not be drawn");
            }
            let message_external_id = press.message_external_id.to_owned();
            // The completion outlives this call, so it carries the site and its own handle to the
            // channel.
            let chat_external_id = site.chat_external_id.to_owned();
            let target = chat.to_owned();
            let reply_access = access.clone();
            tokio::spawn(async move {
                let site = PageSite {
                    chat_external_id: &chat_external_id,
                    ..site
                };
                if delivery == Delivery::Person
                    && let Err(e) = reply_access
                        .notice(&target, &copy::dm_queued(language))
                        .await
                {
                    tracing::warn!(chat_id = %site.chat_id, error = %e, "digest notice could not be delivered");
                }
                match reply.await {
                    Ok(Ok(report)) => {
                        DeliveryJob {
                            access: &*reply_access,
                            pages: &pages,
                            site,
                            message_external_id: &message_external_id,
                            delivery,
                            target: &target,
                        }
                        .send(stretch, &report.text, report.truncated)
                        .await;
                    }
                    Ok(Err(error)) => {
                        let state = DigestDialog::Failed {
                            stretch,
                            error: error.into(),
                            language: report_language,
                            delivery,
                        };
                        draw_page(&*reply_access, &pages, site, &message_external_id, &state).await;
                    }
                    // The chat agent dropped the request; a restart is the ordinary reason.
                    Err(_) => {
                        let state = DigestDialog::Failed {
                            stretch,
                            error: copy::interrupted(language).into(),
                            language: report_language,
                            delivery,
                        };
                        draw_page(&*reply_access, &pages, site, &message_external_id, &state).await;
                    }
                }
            });
        }
        Err(e) => {
            let failed = DigestDialog::Failed {
                stretch,
                error: e.to_string().into(),
                language: report_language,
                delivery,
            };
            draw_page(&*access, &pages, site, press.message_external_id, &failed).await;
        }
    }
    access
        .answer_callback(press.query_id, CallbackAnswer::Quiet)
        .await
}

/// Draws one page where its message stands, tokens fresh; a rewrite the platform refuses goes out
/// as a new message.
async fn draw_page(
    access: &dyn ChannelAccess,
    pages: &Pages,
    site: PageSite<'_>,
    message_external_id: &str,
    state: &DigestDialog,
) {
    let options = page(pages, site, state);
    let text = copy::stage_text(site.language, state);
    if let Err(e) = rewrite_or_ask(access, &site, message_external_id, &text, &options).await {
        tracing::warn!(chat_id = %site.chat_id, error = %e, "digest page could not be delivered");
    }
}

/// Delivers the report an `Undelivered` page kept, to the destination this press names. The press is
/// answered after the attempt.
async fn redeliver(
    pages: &Pages,
    access: &dyn ChannelAccess,
    state: DigestDialog,
    delivery: Delivery,
    press: &Press<'_>,
) -> Result<()> {
    let DigestDialog::Undelivered {
        stretch,
        report,
        truncated,
        ..
    } = &state
    else {
        return stale(access, press).await;
    };
    let (stretch, report, truncated) = (*stretch, report.as_ref(), *truncated);
    let Some(chat) = destination(delivery, press) else {
        return refuse(pages, access, press, &state).await;
    };
    DeliveryJob {
        access,
        pages,
        site: press.site,
        message_external_id: press.message_external_id,
        delivery,
        target: chat,
    }
    .send(stretch, report, truncated)
    .await;
    access
        .answer_callback(press.query_id, CallbackAnswer::Quiet)
        .await
}

/// A person-targeted press needs somebody the platform named: the page is drawn again with fresh
/// tokens and the answer says why nothing ran.
async fn refuse(
    pages: &Pages,
    access: &dyn ChannelAccess,
    press: &Press<'_>,
    state: &DigestDialog,
) -> Result<()> {
    draw_page(access, pages, press.site, press.message_external_id, state).await;
    access
        .answer_callback(
            press.query_id,
            CallbackAnswer::Alert(&copy::no_sender(press.site.language)),
        )
        .await
}

/// What one delivery needs: the page it settles, the channel, and where the report goes.
struct DeliveryJob<'a> {
    access: &'a dyn ChannelAccess,
    pages: &'a Pages,
    site: PageSite<'a>,
    message_external_id: &'a str,
    delivery: Delivery,
    /// The platform chat the document goes to, which `destination` resolved from the press.
    target: &'a str,
}

impl DeliveryJob<'_> {
    /// Sends a written report as a file, with the window it covers as its caption, and takes the
    /// page away once the chat shows the document instead. A platform that refuses the delivery
    /// keeps the report on the page: the buttons offer a retry and the chat itself, so nothing paid
    /// for is lost.
    async fn send(&self, stretch: Stretch, report: &str, truncated: bool) {
        let language = self.site.language;
        let caption = copy::caption(language, stretch, truncated);
        match self
            .access
            .notice_document(
                self.target,
                &copy::filename(stretch),
                report.as_bytes().to_vec(),
                &caption,
            )
            .await
        {
            Ok(()) => {
                if let Err(e) = self
                    .access
                    .delete_notice(self.site.chat_external_id, self.message_external_id)
                    .await
                {
                    tracing::warn!(chat_id = %self.site.chat_id, error = %e, "digest page could not be removed");
                }
            }
            Err(e) => {
                let state = DigestDialog::Undelivered {
                    stretch,
                    report: report.into(),
                    truncated,
                    target: self.delivery,
                    error: e.to_string().into(),
                };
                draw_page(
                    self.access,
                    self.pages,
                    self.site,
                    self.message_external_id,
                    &state,
                )
                .await;
            }
        }
    }
}

/// A press whose page is not the one it should be: the dialog has moved on, so nothing is done.
async fn stale(access: &dyn ChannelAccess, press: &Press<'_>) -> Result<()> {
    let language = press.site.language;
    access
        .answer_callback(
            press.query_id,
            CallbackAnswer::Toast(&HostCopy::DialogStale.render(language)),
        )
        .await
}

/// One page's buttons, each with a token of its own bound to this chat and this state. Anybody in the
/// chat may press them: the report is the chat's own, and a delivery goes to whoever asks.
fn page(pages: &Pages, site: PageSite<'_>, state: &DigestDialog) -> Vec<ChoiceOption> {
    let actions = dialog::options(site.language, site.chat_type, state);
    draw(pages, site, state, actions)
}

/// Binds each action to this state and this chat, one token per button; a page whose buttons are not
/// its own state's (the advice page) goes through here too.
fn draw(
    pages: &Pages,
    site: PageSite<'_>,
    state: &DigestDialog,
    actions: Vec<(String, DigestAction)>,
) -> Vec<ChoiceOption> {
    let buttons: Vec<(String, DialogPage)> = actions
        .into_iter()
        .map(|(label, action)| {
            (
                label,
                DialogPage::Digest {
                    state: state.clone(),
                    action,
                },
            )
        })
        .collect();
    pages.draw(site, Audience::Chat, &buttons)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::{
            inbound::{Arrival, ArrivalChat, ArrivalSender, ArrivalText},
            test_support::RecordingAccess,
        },
        domain::{
            model::{ChatType, Platform},
            repo::ChangeKind,
            vo::EntryAudience,
        },
    };

    const CHAT: ChatId = ChatId(7);
    const WEEK: Stretch = Stretch::Last { secs: 7 * 86_400 };

    fn undelivered(target: Delivery) -> DigestDialog {
        DigestDialog::Undelivered {
            stretch: WEEK,
            report: "the report".into(),
            truncated: false,
            target,
            error: "nowhere to deliver it".into(),
        }
    }

    /// A page in a group, where both destinations are offered.
    fn group_site() -> PageSite<'static> {
        PageSite {
            chat_type: ChatType::Group,
            ..site()
        }
    }

    /// One delivery the way the dialog makes it: the page, the channel and where the report goes.
    fn job<'a>(
        access: &'a RecordingAccess,
        pages: &'a Pages,
        site: PageSite<'a>,
        delivery: Delivery,
        target: &'a str,
    ) -> DeliveryJob<'a> {
        DeliveryJob {
            access,
            pages,
            site,
            message_external_id: "m",
            delivery,
            target,
        }
    }

    /// The chat every test page speaks to.
    fn site() -> PageSite<'static> {
        PageSite {
            chat_id: CHAT,
            chat_external_id: "7",
            chat_type: ChatType::Private,
            language: Language::Zh,
        }
    }

    fn arrival() -> Arrival {
        Arrival {
            platform: Platform::Telegram,
            chat: ArrivalChat {
                external_id: "7".to_owned(),
                kind: ChatType::Private,
                title: None,
            },
            sender: Some(ArrivalSender {
                external_id: "42".to_owned(),
                meta: None,
            }),
            text: "/digest".to_owned(),
            attachments: Vec::new(),
            external_id: "1".to_owned(),
            reply_external_id: None,
            change: ChangeKind::Created { edited_at: None },
            sent_at: None,
            audience: EntryAudience::Agent,
            meta: serde_json::Value::Null,
            text_kind: ArrivalText::Ordinary,
        }
    }

    /// The command's first page: the presets when nobody named a stretch, the confirmation when they
    /// did.
    #[tokio::test]
    async fn the_first_page_follows_what_was_named() {
        let access = RecordingAccess::default();
        let pages = Pages::new();

        open(
            &pages,
            &access,
            CHAT,
            &arrival(),
            DigestArg {
                stretch: None,
                language: None,
            },
            Language::Zh,
        )
        .await
        .expect("the stretch page goes out");
        open(
            &pages,
            &access,
            CHAT,
            &arrival(),
            DigestArg {
                stretch: Some(WEEK),
                language: None,
            },
            Language::En,
        )
        .await
        .expect("the confirmation page goes out");

        let asked = access.asked.lock().expect("test channel lock");
        assert_eq!(asked.len(), 2);
        assert_eq!(asked[0].chat_external_id, "7");
        assert!(asked[0].prompt.contains("时间范围"), "{}", asked[0].prompt);
        assert_eq!(asked[0].options.len(), 5, "three presets, advice and close");
        assert_eq!(asked[1].options.len(), 3, "start, back and close");
        assert!(
            asked[1].prompt.contains("About to write the report"),
            "{}",
            asked[1].prompt
        );
    }

    /// The report language a person named reaches the page they confirm on.
    #[tokio::test]
    async fn a_named_report_language_reaches_the_page() {
        let access = RecordingAccess::default();
        let pages = Pages::new();

        open(
            &pages,
            &access,
            CHAT,
            &arrival(),
            DigestArg {
                stretch: Some(WEEK),
                language: Some(Language::Ja),
            },
            Language::En,
        )
        .await
        .expect("the confirmation page goes out");

        let asked = access.asked.lock().expect("test channel lock");
        assert!(
            asked[0].prompt.contains("Report language:"),
            "{}",
            asked[0].prompt
        );
    }

    /// A report for the presser goes to their private chat, with the window as its caption, and the
    /// dialog page goes away once the document is there.
    #[tokio::test]
    async fn a_dm_report_lands_in_the_pressers_private_chat() {
        let access = RecordingAccess::default();
        let pages = Pages::new();

        job(&access, &pages, site(), Delivery::Person, "42")
            .send(WEEK, "the report", false)
            .await;

        let documents = access.documents.lock().expect("test channel lock");
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].0, "42", "it goes to the person who pressed");
        assert!(documents[0].1.starts_with("digest-"), "{}", documents[0].1);
        assert!(documents[0].1.ends_with(".md"), "{}", documents[0].1);
        assert_eq!(documents[0].2, b"the report");
        assert!(
            documents[0].3.contains('–'),
            "the caption names the window: {}",
            documents[0].3
        );

        let deleted = access.deleted.lock().expect("test channel lock");
        assert_eq!(
            *deleted,
            [("7".to_owned(), "m".to_owned())],
            "the group page goes away; the private message is the only trace"
        );
        assert!(
            access.edited.lock().expect("test channel lock").is_empty(),
            "no receipt is left in the group"
        );
    }

    /// A report for the chat goes out there, and the page goes away with it.
    #[tokio::test]
    async fn a_report_for_the_chat_lands_in_the_chat() {
        let access = RecordingAccess::default();
        let pages = Pages::new();

        job(&access, &pages, site(), Delivery::Chat, "7")
            .send(WEEK, "the report", false)
            .await;

        let documents = access.documents.lock().expect("test channel lock");
        assert_eq!(documents[0].0, "7", "it goes to the dialog's own chat");
        let deleted = access.deleted.lock().expect("test channel lock");
        assert_eq!(
            *deleted,
            [("7".to_owned(), "m".to_owned())],
            "the page goes away once the document is there"
        );
        assert!(
            access.edited.lock().expect("test channel lock").is_empty(),
            "no receipt stands where the page was"
        );
    }

    /// Anybody in the chat may press a digest page: the report is the chat's own, and the copy goes to
    /// whoever pressed.
    #[tokio::test]
    async fn another_member_can_ask_for_the_report() {
        let access = RecordingAccess::default();
        let pages = Pages::new();
        let confirm = DigestDialog::Confirm {
            stretch: WEEK,
            language: None,
        };
        let options = page(&pages, group_site(), &confirm);
        assert_eq!(options.len(), 4, "two destinations, back and close");

        let token = options[0].token.clone();
        let press = Press {
            site: group_site(),
            message_external_id: "m",
            token: &token,
            query_id: "q",
            sender_id: Some("43"),
        };
        let page = pages
            .take(&press)
            .expect("the page admits anybody in the chat");
        let DialogPage::Digest { action, .. } = page else {
            panic!("a digest press reads back a digest page");
        };
        assert_eq!(action, DigestAction::Start(Delivery::Person));

        job(&access, &pages, group_site(), Delivery::Person, "43")
            .send(WEEK, "the report", false)
            .await;

        let documents = access.documents.lock().expect("test channel lock");
        assert_eq!(documents.len(), 1);
        assert_eq!(
            documents[0].0, "43",
            "the copy goes to the presser, not to whoever opened the dialog"
        );
    }

    /// A delivery the platform refuses keeps the report on the page: the private chat gets a retry,
    /// and the chat itself is offered as the way out.
    #[tokio::test]
    async fn an_undeliverable_dm_keeps_the_report_for_a_retry() {
        let access = RecordingAccess {
            document_fails: true,
            ..RecordingAccess::default()
        };
        let pages = Pages::new();

        job(&access, &pages, site(), Delivery::Person, "42")
            .send(WEEK, "the report", false)
            .await;

        assert!(
            access
                .documents
                .lock()
                .expect("test channel lock")
                .is_empty(),
            "nothing was delivered"
        );
        let token = {
            let edited = access.edited.lock().expect("test channel lock");
            assert_eq!(edited.len(), 1, "the page is drawn again");
            assert!(
                edited[0].prompt.contains("私聊"),
                "the page names the private-chat problem: {}",
                edited[0].prompt
            );
            assert_eq!(
                edited[0].options.len(),
                3,
                "retry, the chat itself, and close"
            );
            edited[0].options[0].token.clone()
        };

        let press = Press {
            site: site(),
            message_external_id: "m",
            token: &token,
            query_id: "q",
            sender_id: Some("42"),
        };
        let page = pages
            .take(&press)
            .expect("the retry page admits its presser");
        let DialogPage::Digest { state, action } = page else {
            panic!("a digest press reads back a digest page");
        };
        assert_eq!(action, DigestAction::Deliver(Delivery::Person));
        let DigestDialog::Undelivered {
            stretch,
            report,
            truncated,
            ..
        } = state
        else {
            panic!("the retry page keeps the report");
        };

        let working = RecordingAccess::default();
        job(&working, &pages, site(), Delivery::Person, "42")
            .send(stretch, &report, truncated)
            .await;

        let documents = working.documents.lock().expect("test channel lock");
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].0, "42");
    }

    /// The undelivered page is the only one that keeps the report, so the stored state is what a
    /// retry reads.
    #[tokio::test]
    async fn an_undelivered_page_offers_the_chat_when_the_dm_failed() {
        let pages = Pages::new();
        let options = page(&pages, site(), &undelivered(Delivery::Person));
        let actions: Vec<_> = options
            .iter()
            .filter_map(|option| {
                let page = pages.take(&Press {
                    site: site(),
                    message_external_id: "m",
                    token: &option.token,
                    query_id: "q",
                    sender_id: Some("42"),
                });
                match page {
                    Ok(DialogPage::Digest { action, .. }) => Some(action),
                    _ => None,
                }
            })
            .collect();
        assert_eq!(
            actions,
            vec![
                DigestAction::Deliver(Delivery::Person),
                DigestAction::Deliver(Delivery::Chat),
                DigestAction::Close
            ]
        );

        let chat_only = page(&pages, site(), &undelivered(Delivery::Chat));
        assert_eq!(
            chat_only.len(),
            2,
            "a chat delivery that failed offers retry and close only"
        );
    }
}
