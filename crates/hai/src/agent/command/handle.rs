use super::{
    ArgName, Command, CommandContext, CommandOutcome, ContentPolicyArg, Dialog, DialogArg,
    DialogFallback, DigestArg, DigestStart, LanguageArg, PersonaArg, PurgeArg, Reply, Said,
    Started, SystemCommand,
    parse::{RANGE, YES},
};
use crate::{
    agent::{command::digest::Delivery, host_copy::HostCopy},
    config::conversation::{ContentPolicy, Language},
    domain::vo::{AgentCommand, ChatEntry, EntryAudience, EntryKind, HostAction, PurgeSpec},
    error::Result,
};

/// Answers one command the table has read. The same function serves both ways a command is
/// collected: a command line and a page of buttons both arrive here as the same `Command`, and a
/// complete one reaches `Started` without a page.
pub(crate) async fn execute(ctx: &CommandContext, command: Command) -> Result<CommandOutcome> {
    let outcome = match command {
        Command::System(command) => answer(ctx, command).await?,
        Command::Agent(command) => {
            enqueue(ctx, command).await?;
            CommandOutcome::silent()
        }
    };
    if let Some(action) = outcome.action
        && let Err(e) = ctx.record_action(action).await
    {
        tracing::warn!(%ctx.conversation_id, "host action was not recorded: {e}");
    }
    Ok(outcome)
}

/// Answers a command the host handles itself.
async fn answer(ctx: &CommandContext, command: SystemCommand) -> Result<CommandOutcome> {
    match command {
        SystemCommand::Status => Ok(status(ctx).await?.into()),
        SystemCommand::Lang(arg) => language(ctx, arg).await,
        SystemCommand::ContentPolicy(arg) => content_policy(ctx, arg).await,
        SystemCommand::Persona(arg) => persona(ctx, arg).await,
        SystemCommand::Purge(arg) => match submit_purge(arg) {
            Ok(spec) => Ok(CommandOutcome::started(Started::Purge(spec))),
            Err(missing) => {
                let language = host_language(ctx).await?;
                Ok(CommandOutcome::dialog(Dialog {
                    language,
                    arg: DialogArg::Purge(arg),
                    fallback: DialogFallback::Missing(missing),
                }))
            }
        },
        SystemCommand::Digest(arg) => match submit_digest(arg) {
            Ok(start) => Ok(CommandOutcome::started(Started::Digest(start))),
            Err(missing) => {
                let language = host_language(ctx).await?;
                Ok(CommandOutcome::dialog(Dialog {
                    language,
                    arg: DialogArg::Digest(arg),
                    fallback: DialogFallback::Missing(missing),
                }))
            }
        },
    }
}

/// A purge the command line completed: the range and the confirmation are both present. Anything
/// less names the argument the page would have asked for.
fn submit_purge(arg: PurgeArg) -> Result<PurgeSpec, ArgName> {
    match (arg.range, arg.yes) {
        (Some(range), true) => Ok(PurgeSpec {
            range,
            surfaces: arg.surfaces,
        }),
        (None, _) => Err(ArgName::Positional(RANGE)),
        (Some(_), false) => Err(ArgName::Option(YES)),
    }
}

/// A digest the command line completed: the stretch is present, and the delivery defaults to the
/// chat. A bare `/digest` is a question, not a default report over an unstated window.
fn submit_digest(arg: DigestArg) -> Result<DigestStart, ArgName> {
    match arg.stretch {
        Some(stretch) => Ok(DigestStart {
            stretch,
            delivery: arg.delivery.unwrap_or(Delivery::Chat),
            language: arg.language,
        }),
        None => Err(ArgName::Positional(RANGE)),
    }
}

async fn said(ctx: &CommandContext, copy: HostCopy) -> Result<Said> {
    let language = host_language(ctx).await?;
    Ok(Said { copy, language })
}

async fn host_language(ctx: &CommandContext) -> Result<Language> {
    ctx.settings.language(ctx.conversation_id).await
}

async fn status(ctx: &CommandContext) -> Result<Reply> {
    let language = host_language(ctx).await?;
    let body = match ctx.agents.get(ctx.conversation_id).await {
        Some(agent) => agent.status_text(language).await,
        None => None,
    };
    Ok(Reply::Said(Said {
        copy: HostCopy::Status(body),
        language,
    }))
}

/// A set or reset is applied before the reply; a query and an unsupported code change nothing.
async fn language(ctx: &CommandContext, arg: LanguageArg) -> Result<CommandOutcome> {
    let (copy, changed) = match arg {
        LanguageArg::Current => (HostCopy::LanguageCurrent, false),
        LanguageArg::Reset => {
            ctx.settings.reset_language(ctx.conversation_id).await?;
            (HostCopy::LanguageReset, true)
        }
        LanguageArg::Set(language) => {
            ctx.settings
                .set_language(ctx.conversation_id, language)
                .await?;
            (HostCopy::LanguageSet, true)
        }
        LanguageArg::Unsupported => (HostCopy::LanguageUnsupported, false),
    };
    let outcome = CommandOutcome::said(said(ctx, copy).await?);
    Ok(if changed {
        outcome.with_action(HostAction::LanguageChanged)
    } else {
        outcome
    })
}

/// A set or reset content policy is a change the next Turn has to see, and a stricter one also
/// reopens the chapter through the wrap-up. The bare line opens the picker, or reports the current
/// level where the channel has no page.
async fn content_policy(ctx: &CommandContext, arg: ContentPolicyArg) -> Result<CommandOutcome> {
    let old = ctx.settings.content_policy(ctx.conversation_id).await?;
    match arg {
        ContentPolicyArg::Current => {
            let language = host_language(ctx).await?;
            Ok(CommandOutcome::dialog(content_policy_picker(language, old)))
        }
        ContentPolicyArg::Reset => {
            ctx.settings
                .reset_content_policy(ctx.conversation_id)
                .await?;
            let new = ctx.settings.content_policy(ctx.conversation_id).await?;
            if new.is_stricter_than(old) {
                enqueue_policy_reopen(ctx).await?;
            }
            Ok(
                CommandOutcome::said(said(ctx, HostCopy::ContentPolicyReset(new)).await?)
                    .with_action(HostAction::ContentPolicyChanged),
            )
        }
        ContentPolicyArg::Set(level) => {
            let new = level;
            ctx.settings
                .set_content_policy(ctx.conversation_id, new)
                .await?;
            if new.is_stricter_than(old) {
                enqueue_policy_reopen(ctx).await?;
            }
            Ok(
                CommandOutcome::said(said(ctx, HostCopy::ContentPolicySet(new)).await?)
                    .with_action(HostAction::ContentPolicyChanged),
            )
        }
    }
}

/// The bare `/content-policy` line: the picker where the channel has one, and the same current
/// level as a spoken answer where it has none.
fn content_policy_picker(language: Language, current: ContentPolicy) -> Dialog {
    Dialog {
        language,
        arg: DialogArg::ContentPolicy(current),
        fallback: DialogFallback::Answer(Said {
            copy: HostCopy::ContentPolicyCurrent(current),
            language,
        }),
    }
}

/// A set or reset persona is a change the next Turn has to see; a query and a rejection change
/// nothing and say so.
async fn persona(ctx: &CommandContext, arg: PersonaArg) -> Result<CommandOutcome> {
    match arg {
        PersonaArg::Current => {
            let persona = ctx.settings.persona(ctx.conversation_id).await?;
            Ok(CommandOutcome::said(
                said(ctx, HostCopy::PersonaCurrent(persona)).await?,
            ))
        }
        PersonaArg::Reset => {
            ctx.settings.reset_persona(ctx.conversation_id).await?;
            Ok(
                CommandOutcome::said(said(ctx, HostCopy::PersonaReset).await?)
                    .with_action(HostAction::PersonaChanged),
            )
        }
        PersonaArg::Set(text) => {
            if text.trim().is_empty() {
                return Ok(CommandOutcome::said(
                    said(ctx, HostCopy::PersonaEmpty).await?,
                ));
            }
            if text.chars().count() > 4096 {
                return Ok(CommandOutcome::said(
                    said(ctx, HostCopy::PersonaTooLong).await?,
                ));
            }
            ctx.settings.set_persona(ctx.conversation_id, text).await?;
            Ok(CommandOutcome::said(said(ctx, HostCopy::PersonaSet).await?)
                .with_action(HostAction::PersonaChanged))
        }
    }
}

async fn enqueue_policy_reopen(ctx: &CommandContext) -> Result<()> {
    let entry = ChatEntry::new(
        ctx.chat_id,
        EntryAudience::Agent,
        EntryKind::PolicyChanged,
        jiff::Timestamp::now(),
    );
    ctx.services
        .conversation
        .append_entry(ctx.conversation_id, &entry)
        .await?;
    ctx.agents
        .get_or_create(ctx.conversation_id)
        .await
        .notify_chat(ctx.chat_id);
    Ok(())
}

/// Hands the agent an instruction and stays quiet — the answer arrives in the conversation.
async fn enqueue(ctx: &CommandContext, command: AgentCommand) -> Result<()> {
    let entry = ChatEntry::new(
        ctx.chat_id,
        EntryAudience::Agent,
        EntryKind::Command(command),
        jiff::Timestamp::now(),
    );
    ctx.services
        .conversation
        .append_entry(ctx.conversation_id, &entry)
        .await?;
    ctx.agents
        .get_or_create(ctx.conversation_id)
        .await
        .notify_chat(ctx.chat_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::command::Invocation,
        config::conversation::ContentPolicy,
        domain::vo::{PurgeSurfaces, Stretch},
    };

    const WEEK: Stretch = Stretch::Last { secs: 7 * 86_400 };

    /// A purge is complete only when the range and the confirmation are both written; either one
    /// alone is still a question.
    #[test]
    fn a_purge_needs_both_its_range_and_its_confirmation() {
        let surfaces = PurgeSurfaces::all();
        assert!(
            submit_purge(PurgeArg {
                range: Some(WEEK),
                surfaces,
                yes: true,
            },)
            .is_ok()
        );
        assert_eq!(
            submit_purge(PurgeArg {
                range: Some(WEEK),
                surfaces,
                yes: false,
            },)
            .unwrap_err(),
            ArgName::Option(YES)
        );
        assert_eq!(
            submit_purge(PurgeArg {
                range: None,
                surfaces,
                yes: true,
            },)
            .unwrap_err(),
            ArgName::Positional(RANGE)
        );
    }

    /// A digest is complete once its window is written; the delivery falls back to the chat, so the
    /// shortest complete form is still a full command.
    #[test]
    fn a_digest_needs_its_range_and_defaults_to_the_chat() {
        let start = submit_digest(DigestArg {
            stretch: Some(WEEK),
            language: None,
            delivery: None,
        })
        .expect("a complete digest");
        assert_eq!(start.delivery, Delivery::Chat);
        assert_eq!(start.language, None);

        let start = submit_digest(DigestArg {
            stretch: Some(WEEK),
            language: Some(Language::Zh),
            delivery: Some(Delivery::Person),
        })
        .expect("a complete digest");
        assert_eq!(start.delivery, Delivery::Person);
        assert_eq!(start.language, Some(Language::Zh));

        assert_eq!(
            submit_digest(DigestArg {
                stretch: None,
                language: None,
                delivery: None,
            })
            .unwrap_err(),
            ArgName::Positional(RANGE)
        );
    }

    /// A bare `/content-policy` line answers with its current level where the channel cannot show a
    /// page; the picker only adds buttons to that same answer.
    #[test]
    fn a_bare_content_policy_line_answers_where_no_page_exists() {
        let dialog = content_policy_picker(Language::Zh, ContentPolicy::Standard);
        assert_eq!(
            dialog.arg,
            DialogArg::ContentPolicy(ContentPolicy::Standard),
            "the page carries the level it reports"
        );
        match dialog.fallback {
            DialogFallback::Answer(Said { copy, language }) => {
                assert_eq!(language, Language::Zh);
                assert_eq!(
                    copy,
                    HostCopy::ContentPolicyCurrent(ContentPolicy::Standard)
                );
            }
            DialogFallback::Missing(_) => {
                panic!("a complete line answers instead of naming an argument")
            }
        }
    }

    /// The paged commands show at least one form that really runs, not only forms that stop at a
    /// missing argument.
    #[test]
    fn the_paged_commands_show_a_complete_example() {
        for declared in super::super::spec::DECLARATIONS
            .iter()
            .filter(|declared| matches!(declared.name, "purge" | "digest"))
        {
            let complete =
                declared
                    .examples
                    .iter()
                    .any(|example| match (declared.parse)(example) {
                        Invocation::Ready(Command::System(SystemCommand::Purge(arg))) => {
                            submit_purge(arg).is_ok()
                        }
                        Invocation::Ready(Command::System(SystemCommand::Digest(arg))) => {
                            submit_digest(arg).is_ok()
                        }
                        _ => false,
                    });
            assert!(complete, "{} shows no complete example", declared.name);
        }
    }
}
