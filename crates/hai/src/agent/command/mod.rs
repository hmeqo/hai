//! Command semantics: what each command is, what a payload denotes, and what the host says back.

mod args;
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
pub(crate) use spec::{DECLARATIONS, find};

use crate::{
    agent::{command::digest::Delivery, host_copy::HostCopy},
    agentcore::Language,
    domain::vo::{AgentCommand, PurgeSpec, PurgeSurfaces, Stretch},
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
    Purge(PurgeArg),
    Digest(DigestArg),
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Dialog {
    pub(crate) language: Language,
    pub(crate) arg: DialogArg,
    /// The argument the line stopped at, so a channel without pages can report the same refusal a
    /// command line would instead of inventing UI copy.
    pub(crate) missing: ArgName,
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
