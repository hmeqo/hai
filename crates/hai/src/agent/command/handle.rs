use super::{
    ArgName, Command, CommandContext, Dialog, DialogArg, DigestArg, DigestStart, LanguageArg,
    PurgeArg, Reply, Said, Started, SystemCommand,
    parse::{RANGE, YES},
};
use crate::{
    agent::{command::digest::Delivery, host_copy::HostCopy},
    agentcore::Language,
    domain::vo::{AgentCommand, ChatEntry, EntryAudience, EntryKind, PurgeSpec},
    error::Result,
};

/// Answers one command the table has read. The same function serves both ways a command is
/// collected: a command line and a page of buttons both arrive here as the same `Command`, and a
/// complete one reaches `Started` without a page.
pub(crate) async fn execute(ctx: &CommandContext, command: Command) -> Result<Reply> {
    match command {
        Command::System(command) => answer(ctx, command).await,
        Command::Agent(command) => {
            enqueue(ctx, command).await?;
            Ok(Reply::Silent)
        }
    }
}

/// Answers a command the host handles itself.
async fn answer(ctx: &CommandContext, command: SystemCommand) -> Result<Reply> {
    match command {
        SystemCommand::Status => status(ctx).await,
        SystemCommand::Lang(arg) => language(ctx, arg).await,
        SystemCommand::Purge(arg) => match submit_purge(arg) {
            Ok(spec) => Ok(Reply::Started(Started::Purge(spec))),
            Err(missing) => {
                let language = host_language(ctx).await?;
                Ok(Reply::Dialog(Dialog {
                    language,
                    arg: DialogArg::Purge(arg),
                    missing,
                }))
            }
        },
        SystemCommand::Digest(arg) => match submit_digest(arg) {
            Ok(start) => Ok(Reply::Started(Started::Digest(start))),
            Err(missing) => {
                let language = host_language(ctx).await?;
                Ok(Reply::Dialog(Dialog {
                    language,
                    arg: DialogArg::Digest(arg),
                    missing,
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

async fn said(ctx: &CommandContext, copy: HostCopy) -> Result<Reply> {
    let language = host_language(ctx).await?;
    Ok(Reply::Said(Said { copy, language }))
}

async fn host_language(ctx: &CommandContext) -> Result<Language> {
    ctx.settings.language(ctx.chat_id).await
}

async fn status(ctx: &CommandContext) -> Result<Reply> {
    let language = host_language(ctx).await?;
    let body = match ctx.agents.get(ctx.chat_id).await {
        Some(agent) => agent.status_text(language).await,
        None => None,
    };
    Ok(Reply::Said(Said {
        copy: HostCopy::Status(body),
        language,
    }))
}

/// The preference is written before the reply is spoken, so a set language is confirmed in the
/// language it just became.
async fn language(ctx: &CommandContext, arg: LanguageArg) -> Result<Reply> {
    let copy = match arg {
        LanguageArg::Current => HostCopy::LanguageCurrent,
        LanguageArg::Reset => {
            ctx.settings.reset_language(ctx.chat_id).await?;
            HostCopy::LanguageReset
        }
        LanguageArg::Set(language) => {
            ctx.settings.set_language(ctx.chat_id, language).await?;
            HostCopy::LanguageSet
        }
        LanguageArg::Unsupported => HostCopy::LanguageUnsupported,
    };
    said(ctx, copy).await
}

/// Hands the agent an instruction and stays quiet — the answer arrives in the conversation.
async fn enqueue(ctx: &CommandContext, command: AgentCommand) -> Result<()> {
    let entry = ChatEntry::new(
        EntryAudience::Agent,
        EntryKind::Command(command),
        jiff::Timestamp::now(),
    );
    ctx.services
        .conversation
        .append_entry(ctx.chat_id, &entry)
        .await?;
    ctx.agents.get_or_create(ctx.chat_id).await.notify();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::command::{DECLARATIONS, Invocation},
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
            })
            .is_ok()
        );
        assert_eq!(
            submit_purge(PurgeArg {
                range: Some(WEEK),
                surfaces,
                yes: false,
            })
            .unwrap_err(),
            ArgName::Option(YES)
        );
        assert_eq!(
            submit_purge(PurgeArg {
                range: None,
                surfaces,
                yes: true,
            })
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

    /// The paged commands show at least one form that really runs, not only forms that stop at a
    /// missing argument.
    #[test]
    fn the_paged_commands_show_a_complete_example() {
        for declared in DECLARATIONS
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
