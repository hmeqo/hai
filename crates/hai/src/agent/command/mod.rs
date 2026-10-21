//! Command semantics: what each command is, what a payload denotes, and what the host says back.

mod args;
mod context;
pub(crate) mod dialog;
pub(crate) mod digest;
mod flags;
mod handle;
mod menu;
mod parse;
pub(crate) mod purge;
mod spec;

pub(crate) use args::{ArgError, Grammar};
pub(crate) use context::CommandContext;
pub(crate) use handle::execute;
pub(crate) use parse::{MAX_DIGEST_DAYS, MIN_DIGEST_DAYS};
pub(crate) use spec::{DECLARATIONS, find};

use crate::{
    agent::host_copy::HostCopy,
    agentcore::Language,
    domain::vo::{AgentCommand, PurgeSurfaces, Stretch},
};

/// A payload the table has judged: either a command it could read, or one it could not, with the
/// reason the person is owed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Invocation {
    Ready(Command),
    Unreadable(ArgError),
}

/// A command the host answers itself, without the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SystemCommand {
    Status,
    Lang(LanguageArg),
    Purge(PurgeArg),
    Digest(DigestArg),
}

/// What `/digest` was asked for: the stretch a person named (absent while they have not chosen one)
/// and the language of the one report it writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DigestArg {
    pub stretch: Option<Stretch>,
    /// The language this report is written in, when it was named; absent means the chat's.
    pub language: Option<Language>,
}

/// What `/purge` was asked for: the stretch a person named (absent while they have not chosen one)
/// and the surfaces it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PurgeArg {
    pub range: Option<Stretch>,
    pub surfaces: PurgeSurfaces,
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
}

/// A confirmation to hold with the platform before the command's work is enqueued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Dialog {
    pub(crate) language: Language,
    pub(crate) arg: DialogArg,
}

/// What the dialog is a question about: how far the command got before it needed a person to decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DialogArg {
    Purge(PurgeArg),
    Digest(DigestArg),
}

/// One reply the host speaks, in the language it has to be spoken in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Said {
    pub(crate) copy: HostCopy,
    pub(crate) language: Language,
}
