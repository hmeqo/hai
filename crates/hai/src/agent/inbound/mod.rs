//! The one inbound path of a channel: whatever a platform delivers becomes a stored change, an
//! answer the host composes, or nothing at all. A platform reads its own updates into an `Arrival`
//! and hands it here.

mod arrival;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(crate) use arrival::{Arrival, ArrivalAttachment, ArrivalChat, ArrivalSender, ArrivalText};

use crate::{
    agent::{
        channel::{CallbackAnswer, ChannelAccess, ChannelChoices},
        command::{
            ArgError, CommandContext, DialogArg, DialogFallback, Invocation, Reply, Started,
            content_policy,
            dialog::{DialogPage, Miss, PageSite, Pages, Press},
            digest, execute, find, purge,
        },
        host_copy::HostCopy,
        process::registry::AgentRegistry,
        settings::ConversationSettings,
    },
    config::conversation::{ConversationScope, Language},
    domain::{
        model::{Chat, ChatLocation, NewAttachment, Platform},
        repo::PlatformChange,
        service::Services,
        vo::{ChannelKey, ChatId},
    },
    error::{AppError, Result},
};

/// What an unauthorized message does: its greeting is answered once per cooldown, and nothing else
/// of its is. It is asked before anything is stored, and registers nothing — a chat outside the
/// allowlist is not ours to store.
pub(crate) fn answers_unauthorized(text_kind: &ArrivalText, answered_recently: bool) -> bool {
    matches!(text_kind, ArrivalText::Greeting) && !answered_recently
}

/// Unauthorized-chat replies: one answer per chat per cooldown, and a bounded tracking map.
struct UnauthorizedReplies {
    seen: Mutex<HashMap<String, Instant>>,
}

const UNAUTHORIZED_REPLY_COOLDOWN: Duration = Duration::from_secs(3600);
const UNAUTHORIZED_TRACKED_CHATS: usize = 1024;

impl UnauthorizedReplies {
    fn new() -> Self {
        Self {
            seen: Mutex::new(HashMap::new()),
        }
    }

    /// Whether this chat may be answered now.
    fn claim(&self, chat_external_id: &str) -> bool {
        let now = Instant::now();
        let mut seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        seen.retain(|_, at| now.duration_since(*at) < UNAUTHORIZED_REPLY_COOLDOWN);
        if seen.contains_key(chat_external_id) || seen.len() >= UNAUTHORIZED_TRACKED_CHATS {
            return false;
        }
        seen.insert(chat_external_id.to_owned(), now);
        true
    }
}

pub(crate) struct Intake {
    channel_key: ChannelKey,
    scope: ConversationScope,
    access: Arc<dyn ChannelAccess>,
    /// The button surface and its pending pages; absent on a channel that cannot show a page.
    choices: Option<ChoiceSurface>,
    services: Services,
    settings: ConversationSettings,
    /// What host copy goes out in for a chat that has no row and no preference to read.
    default_language: Language,
    registry: AgentRegistry,
    unauthorized: UnauthorizedReplies,
}

/// What a channel with a button surface owns: the platform's page calls and the pages waiting for a
/// press. A channel without the surface never allocates either.
struct ChoiceSurface {
    choices: Arc<dyn ChannelChoices>,
    pages: Arc<Pages>,
}

/// Everything one channel hands its inbound tail at assembly time.
pub(crate) struct IntakeParts {
    pub channel_key: ChannelKey,
    pub scope: ConversationScope,
    pub access: Arc<dyn ChannelAccess>,
    pub choices: Option<Arc<dyn ChannelChoices>>,
    pub services: Services,
    pub settings: ConversationSettings,
    pub default_language: Language,
    pub registry: AgentRegistry,
}

impl Intake {
    pub(crate) fn new(parts: IntakeParts) -> Self {
        let IntakeParts {
            channel_key,
            scope,
            access,
            choices,
            services,
            settings,
            default_language,
            registry,
        } = parts;
        Self {
            channel_key,
            scope,
            access,
            choices: choices.map(|choices| ChoiceSurface {
                choices,
                pages: Arc::new(Pages::new()),
            }),
            services,
            settings,
            default_language,
            registry,
            unauthorized: UnauthorizedReplies::new(),
        }
    }

    pub(crate) async fn accept(&self, arrival: Arrival) -> Result<()> {
        if !self.access.admits(&arrival.chat.address) {
            tracing::warn!(
                "Unauthorized message attempt from chat_id: {}, user: {:?}",
                arrival.chat.address,
                arrival.sender.as_ref().map(|sender| &sender.external_id)
            );
            // The claim is spent on every unauthorized arrival, answered or not: one reply per chat
            // per hour is the whole budget.
            let answered_recently = !self.unauthorized.claim(&arrival.chat.address.to_string());
            if answers_unauthorized(&arrival.text_kind, answered_recently) {
                self.notice(&arrival, HostCopy::Unauthorized, self.default_language)
                    .await?;
            }
            return Ok(());
        }

        match &arrival.text_kind {
            ArrivalText::Greeting => {
                let chat = self.ensure_chat(&arrival).await?;
                let language = self.settings.language(chat.conversation_id).await?;
                self.notice(&arrival, HostCopy::Greeting, language).await
            }
            ArrivalText::Command { name, payload } => {
                self.run_command(&arrival, name, payload).await
            }
            ArrivalText::Ordinary => self.store(&arrival).await,
            ArrivalText::Choice {
                token,
                query_id,
                message_external_id,
            } => match &self.choices {
                Some(surface) => {
                    self.resolve_choice(surface, &arrival, token, query_id, message_external_id)
                        .await
                }
                None => unreachable!(
                    "a choice arrival is produced only by a platform that supplies ChannelChoices"
                ),
            },
        }
    }

    /// Host copy is delivered, never stored: the conversation the model reads is the one people had
    /// with it, not the notices around it.
    async fn notice(&self, arrival: &Arrival, copy: HostCopy, language: Language) -> Result<()> {
        self.access
            .notice(&arrival.chat.address, &copy.render(language))
            .await
    }

    /// Registers the chat without its sender: a command carries no message, so no account is needed.
    async fn ensure_chat(&self, arrival: &Arrival) -> Result<Chat> {
        self.services
            .platform
            .get_or_create_chat(
                ChatLocation {
                    channel_key: &self.channel_key,
                    platform: arrival.platform,
                    address: &arrival.chat.address,
                    scope: self.scope,
                },
                arrival.chat.kind,
                arrival.chat.title.as_deref(),
            )
            .await
    }

    async fn run_command(&self, arrival: &Arrival, name: &str, payload: &str) -> Result<()> {
        // A slash message we own but do not know is ordinary conversation.
        let Some(declared) = find(name, self.settings.command_gates()) else {
            return self.store(arrival).await;
        };
        let chat = self.ensure_chat(arrival).await?;
        // The prefix is the platform's; the command spelling is the one the person used.
        let prefix = self.access.command_prefix();
        let usage = || format!("{prefix}{}", declared.synopsis_as(name));
        let command = match (declared.parse)(payload) {
            Invocation::Help => {
                let language = self.settings.language(chat.conversation_id).await?;
                return self
                    .notice(
                        arrival,
                        HostCopy::Usage {
                            about: &declared.copy,
                            usage: usage(),
                            examples: declared.example_lines_as(prefix, name),
                        },
                        language,
                    )
                    .await;
            }
            Invocation::Unreadable(error) => {
                let language = self.settings.language(chat.conversation_id).await?;
                return self
                    .notice(
                        arrival,
                        HostCopy::BadArgs {
                            error,
                            usage: usage(),
                        },
                        language,
                    )
                    .await;
            }
            Invocation::Ready(command) => command,
        };
        let ctx = CommandContext {
            chat_id: chat.id,
            conversation_id: chat.conversation_id,
            settings: self.settings.clone(),
            agents: self.registry.clone(),
            services: self.services.clone(),
        };
        let outcome = execute(&ctx, command).await?;
        match outcome.reply {
            Reply::Said(said) => {
                self.access
                    .notice(&arrival.chat.address, &said.copy.render(said.language))
                    .await?;
            }
            // A command that stopped before its last decision: a page finishes it where the platform
            // has one, and the dialog's own fallback answers everywhere else.
            Reply::Dialog(dialog) => {
                let Some(surface) = &self.choices else {
                    return match dialog.fallback {
                        // No page to finish the line: the missing argument is the answer, exactly as
                        // a command line reports it.
                        DialogFallback::Missing(missing) => {
                            let language = self.settings.language(chat.conversation_id).await?;
                            let error = ArgError::MissingArgument(missing);
                            self.notice(
                                arrival,
                                HostCopy::BadArgs {
                                    error,
                                    usage: usage(),
                                },
                                language,
                            )
                            .await
                        }
                        // The line already had its answer; only the buttons are missing.
                        DialogFallback::Answer(said) => {
                            self.notice(arrival, said.copy, said.language).await
                        }
                    };
                };
                match dialog.arg {
                    DialogArg::Purge(arg) => {
                        purge::open(
                            &surface.pages,
                            &*surface.choices,
                            chat.id,
                            arrival,
                            arg,
                            dialog.language,
                        )
                        .await?;
                    }
                    DialogArg::Digest(arg) => {
                        digest::open(
                            &surface.pages,
                            &*surface.choices,
                            chat.id,
                            arrival,
                            arg,
                            dialog.language,
                        )
                        .await?;
                    }
                    DialogArg::ContentPolicy(current) => {
                        content_policy::open(
                            &surface.pages,
                            &*surface.choices,
                            chat.id,
                            arrival,
                            current,
                            dialog.language,
                        )
                        .await?;
                    }
                }
            }
            Reply::Started(started) => self.start(&ctx, arrival, started).await?,
            Reply::Silent => {}
        }
        Ok(())
    }

    /// Submits the work a complete command line named. A page reaches these same functions with the
    /// same request; the difference is only that the page also repaints itself.
    async fn start(&self, ctx: &CommandContext, arrival: &Arrival, started: Started) -> Result<()> {
        let language = self.settings.language(ctx.conversation_id).await?;
        match started {
            Started::Purge(spec) => {
                purge::queue(ctx, spec).await?;
                self.access
                    .notice(
                        &arrival.chat.address,
                        &purge::copy::started(language, &spec),
                    )
                    .await
            }
            Started::Digest(start) => {
                let origin = arrival.chat.address.clone();
                let delivery = start.delivery.for_chat(arrival.chat.kind);
                let target = match delivery {
                    digest::Delivery::Chat => origin.clone(),
                    digest::Delivery::Person => match arrival.sender.as_ref() {
                        Some(sender) => {
                            self.access.private_chat_external_id(&sender.external_id)?
                        }
                        None => {
                            return self
                                .access
                                .notice(&origin, &digest::copy::no_sender(language))
                                .await;
                        }
                    },
                };
                digest::start(
                    ctx,
                    self.access.clone(),
                    digest::Request {
                        stretch: start.stretch,
                        report_language: start.language,
                        delivery,
                        origin,
                        target,
                        ui_language: language,
                    },
                    None,
                )
                .await
            }
        }
    }

    /// One button press: the chat is registered without its sender (a press carries no message), and
    /// the presser's platform id is only what the page's audience is checked against.
    async fn resolve_choice(
        &self,
        surface: &ChoiceSurface,
        arrival: &Arrival,
        token: &str,
        query_id: &str,
        message_external_id: &str,
    ) -> Result<()> {
        let chat = self.ensure_chat(arrival).await?;
        let language = self.settings.language(chat.conversation_id).await?;
        let sender_id = arrival
            .sender
            .as_ref()
            .map(|sender| sender.external_id.as_str());
        let press = Press {
            site: PageSite {
                chat_id: chat.id,
                chat: &arrival.chat.address,
                chat_type: arrival.chat.kind,
                language,
            },
            message_external_id,
            token,
            query_id,
            sender_id,
        };
        let page = match surface.pages.take(&press) {
            Ok(page) => page,
            Err(miss) => {
                let copy = match miss {
                    Miss::Stale => HostCopy::DialogStale,
                    Miss::Foreign => HostCopy::DialogForeign,
                };
                return surface
                    .choices
                    .answer_callback(query_id, CallbackAnswer::Toast(&copy.render(language)))
                    .await;
            }
        };
        let ctx = CommandContext {
            chat_id: chat.id,
            conversation_id: chat.conversation_id,
            settings: self.settings.clone(),
            agents: self.registry.clone(),
            services: self.services.clone(),
        };
        match page {
            DialogPage::Purge { state, action } => {
                purge::apply(
                    &ctx,
                    &surface.pages,
                    &*surface.choices,
                    state,
                    action,
                    &press,
                )
                .await
            }
            DialogPage::Digest { state, action } => {
                digest::apply(
                    digest::DialogSurface {
                        pages: surface.pages.clone(),
                        access: self.access.clone(),
                        choices: surface.choices.clone(),
                    },
                    &ctx,
                    state,
                    action,
                    &press,
                )
                .await
            }
            DialogPage::ContentPolicy { action } => {
                content_policy::apply(&ctx, &*surface.choices, action, &press).await
            }
        }
    }

    /// The conversation path: resolve the chat and its sender, then let one transaction write the
    /// message row and its accumulated entry before the agent is rung.
    async fn store(&self, arrival: &Arrival) -> Result<()> {
        let sender = arrival.sender.as_ref().ok_or_else(|| {
            AppError::message(format!(
                "arrival {} carries no sender to store",
                arrival.external_id
            ))
        })?;
        let (chat, account) = self
            .services
            .platform
            .ensure_chat_and_account(
                ChatLocation {
                    channel_key: &self.channel_key,
                    platform: arrival.platform,
                    address: &arrival.chat.address,
                    scope: self.scope,
                },
                arrival.chat.kind,
                arrival.chat.title.as_deref(),
                &sender.external_id,
                sender.meta.clone(),
            )
            .await?;
        let attachments = attachment_rows(
            arrival.platform,
            chat.id,
            &arrival.external_id,
            &arrival.attachments,
        );
        self.services
            .platform
            .apply_platform_change(PlatformChange {
                chat_id: chat.id,
                conversation_id: chat.conversation_id,
                account_id: account.id,
                text: arrival.text.clone(),
                attachments,
                external_id: arrival.external_id.clone(),
                meta: arrival.meta.clone(),
                sent_at: arrival.sent_at,
                kind: arrival.change,
                reply_external_id: arrival.reply_external_id.clone(),
                audience: arrival.audience,
            })
            .await?;
        self.registry
            .get_or_create(chat.conversation_id)
            .await
            .notify_chat(chat.id);
        Ok(())
    }
}

/// The rows one arrival's files become. The occurrence id's namespace is the chat's own id, which is
/// the same input the outbound path gives `NewAttachment::new`.
fn attachment_rows(
    platform: Platform,
    chat_id: ChatId,
    external_id: &str,
    attachments: &[ArrivalAttachment],
) -> Vec<NewAttachment> {
    attachments
        .iter()
        .enumerate()
        .map(|(ordinal, attachment)| {
            let ordinal = i32::try_from(ordinal).expect("a message carries fewer than 2^31 files");
            NewAttachment::new(
                platform,
                ordinal,
                chat_id,
                external_id,
                attachment.kind,
                attachment.file_unique_id.clone(),
                attachment.file_id.clone(),
                attachment.caption.clone(),
                attachment.meta.clone(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::vo::{AttachmentKind, AttachmentMeta, attachment_id};

    /// The occurrence id is namespaced by the chat's own id — not by the platform's chat id, which
    /// is all the platform layer ever sees.
    #[test]
    fn an_arrivals_files_get_the_chats_own_namespace() {
        let chat = ChatId(900_001);
        let rows = attachment_rows(
            Platform::Telegram,
            chat,
            "77",
            &[ArrivalAttachment {
                kind: AttachmentKind::Photo,
                file_unique_id: Some("uniq".to_owned()),
                file_id: "handle".to_owned(),
                caption: Some("the line".to_owned()),
                meta: AttachmentMeta::default(),
            }],
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, attachment_id("telegram", chat, "77", 0));
        assert_eq!(rows[0].ordinal, 0);
        assert_eq!(rows[0].file_unique_id.as_deref(), Some("uniq"));
        assert_eq!(rows[0].file_id, "handle");
    }

    #[test]
    fn only_an_unauthorized_greeting_inside_its_cooldown_is_answered() {
        assert!(answers_unauthorized(&ArrivalText::Greeting, false));
        assert!(
            !answers_unauthorized(&ArrivalText::Greeting, true),
            "one answer per hour"
        );

        let command = ArrivalText::Command {
            name: "status".to_owned(),
            payload: String::new(),
        };
        for answered_recently in [false, true] {
            assert!(!answers_unauthorized(
                &ArrivalText::Ordinary,
                answered_recently
            ));
            assert!(!answers_unauthorized(&command, answered_recently));
        }
    }

    #[test]
    fn one_reply_per_chat_per_cooldown() {
        let replies = UnauthorizedReplies::new();
        assert!(replies.claim("7"));
        assert!(!replies.claim("7"), "the same chat is not answered twice");
        assert!(replies.claim("8"), "another chat has its own budget");
    }

    #[test]
    fn the_tracking_map_admits_up_to_its_cap() {
        let replies = UnauthorizedReplies::new();
        let admitted: Vec<bool> = (0..UNAUTHORIZED_TRACKED_CHATS)
            .map(|chat| replies.claim(&ChatId(chat as i64 + 1).to_string()))
            .collect();

        assert!(
            admitted.iter().all(|admitted| *admitted),
            "every chat up to the cap is admitted once"
        );
        assert!(
            !replies.claim(&ChatId(UNAUTHORIZED_TRACKED_CHATS as i64 + 1).to_string()),
            "the chat past the cap is refused"
        );
        assert!(!replies.claim("1"), "a tracked chat is not answered twice");
    }
}
