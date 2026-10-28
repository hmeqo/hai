//! Command semantics: what each command is, what a payload denotes, and what the host says back.

mod args;
pub(crate) mod content_policy;
mod context;
pub(crate) mod dialog;
pub(crate) mod digest;
mod handle;
mod menu;
mod parse;
pub(crate) mod purge;
mod spec;

pub(crate) use args::{ArgError, ArgName, Grammar, OptionSpec, Parsed, Positional};
pub(crate) use context::CommandContext;
pub(crate) use handle::execute;
pub(crate) use parse::{MAX_DIGEST_DAYS, MIN_DIGEST_DAYS};
pub(crate) use spec::{CommandGates, Declaration, declarations, find};

use crate::{
    agent::{command::digest::Delivery, host_copy::HostCopy},
    config::conversation::{ContentPolicy, Language},
    domain::vo::{AgentCommand, HostAction, PurgeSpec, PurgeSurfaces, Stretch},
};

/// A payload the table has judged: a command it could read, one it could not with the reason the
/// person is owed, or the form it answers `--help` with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Invocation {
    Ready(Command),
    Unreadable(ArgError),
    Help,
}

/// A command the host answers itself, without the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SystemCommand {
    Status,
    Lang(LanguageArg),
    ContentPolicy(ContentPolicyArg),
    Persona(PersonaArg),
    Purge(PurgeArg),
    Digest(DigestArg),
}

/// What `/content-policy` was asked to do; the level is the typed spelling the parser accepts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ContentPolicyArg {
    Current,
    Reset,
    Set(ContentPolicy),
}

/// What `/persona` was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PersonaArg {
    Current,
    Reset,
    Set(String),
}

/// What `/digest` was asked for: the stretch a person named (absent while they have not chosen one),
/// the language of the one report it writes, and where the report goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DigestArg {
    pub stretch: Option<Stretch>,
    /// The language this report is written in, when it was named; absent means the chat's.
    pub language: Option<Language>,
    /// Where the report goes, when the command line named it; absent means the chat.
    pub delivery: Option<Delivery>,
}

/// What `/purge` was asked for: the stretch a person named, the surfaces it holds, and whether the
/// command line already carried the confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PurgeArg {
    pub range: Option<Stretch>,
    pub surfaces: PurgeSurfaces,
    pub yes: bool,
}

/// A command the host answers itself, or an instruction it hands the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    System(SystemCommand),
    Agent(AgentCommand),
}

/// What `/lang` was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LanguageArg {
    Current,
    Reset,
    Set(Language),
    Unsupported,
}

/// What the host replies with. An agent task is answered by the agent inside the conversation, so
/// the host itself says nothing — `Silent` is that silence, not a missing case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Reply {
    Silent,
    Said(Said),
    /// The answer is a question: the host puts a page of buttons out and reads the presses back. What
    /// the range and the surfaces mean is settled before this leaves the agent layer; how a person is
    /// asked belongs to the platform.
    Dialog(Dialog),
    /// The command line was complete: the action is submitted without a page, and every platform can
    /// carry it.
    Started(Started),
}

/// What one command produced: the reply the host owes and, when the command changed a setting, the
/// change the next Turn has to see. `execute` appends it after the handler; the setting write has
/// already committed, so an append failure is logged and does not turn the command into an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandOutcome {
    pub(crate) reply: Reply,
    pub(crate) action: Option<HostAction>,
}

impl CommandOutcome {
    pub(crate) fn silent() -> Self {
        Reply::Silent.into()
    }

    pub(crate) fn said(said: Said) -> Self {
        Reply::Said(said).into()
    }

    pub(crate) fn dialog(dialog: Dialog) -> Self {
        Reply::Dialog(dialog).into()
    }

    pub(crate) fn started(started: Started) -> Self {
        Reply::Started(started).into()
    }

    /// The setting the command changed, for the next Turn to read.
    pub(crate) fn with_action(mut self, action: HostAction) -> Self {
        self.action = Some(action);
        self
    }
}

impl From<Reply> for CommandOutcome {
    fn from(reply: Reply) -> Self {
        Self {
            reply,
            action: None,
        }
    }
}

/// The work a complete command line submits. A dialog resolves to the same value, so a page is only
/// how the same request was collected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Started {
    Purge(PurgeSpec),
    Digest(DigestStart),
}

/// A digest the command line fully named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DigestStart {
    pub(crate) stretch: Stretch,
    pub(crate) delivery: Delivery,
    pub(crate) language: Option<Language>,
}

/// A confirmation to hold with the platform before the command's work is enqueued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Dialog {
    pub(crate) language: Language,
    pub(crate) arg: DialogArg,
    /// What a channel without pages answers: the argument the line stopped at, or the reply the
    /// command already composed for a bare line.
    pub(crate) fallback: DialogFallback,
}

/// The answer a page would have given, on a channel that cannot show one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DialogFallback {
    /// The line stopped at an argument; the command line reports the same missing argument.
    Missing(ArgName),
    /// The line already had its answer; a page only adds the buttons.
    Answer(Said),
}

/// What the dialog is a question about: how far the command got before it needed a person to decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialogArg {
    Purge(PurgeArg),
    Digest(DigestArg),
    /// The bare content-policy line: the picker reports this policy and offers the levels.
    ContentPolicy(ContentPolicy),
}

/// One reply the host speaks, in the language it has to be spoken in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Said {
    pub(crate) copy: HostCopy,
    pub(crate) language: Language,
}
