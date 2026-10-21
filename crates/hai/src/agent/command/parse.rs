//! What each command's payload denotes: the grammar read, the values checked, the invocation built.

use crate::{
    agent::command::{
        ArgError, Command, DigestArg, Grammar, Invocation, LanguageArg, PurgeArg, SystemCommand,
        args::{FlagName, NoFlags, parse},
        flags::{DigestFlag, LangFlag, PurgeFlag},
    },
    agentcore::Language,
    domain::vo::{AgentCommand, PurgeSurface, PurgeSurfaces, Stretch},
};

/// A `/digest` day count covers at least one day: digesting none of them is not a request.
pub(crate) const MIN_DIGEST_DAYS: u32 = 1;

/// The most days `/digest` runs over at once.
pub(crate) const MAX_DIGEST_DAYS: u32 = 365;

/// A command that takes no argument at all.
pub(super) fn unit(payload: &str, command: Command) -> Invocation {
    match parse::<NoFlags>(payload, &Grammar::None) {
        Ok(_) => Invocation::Ready(command),
        Err(err) => Invocation::Unreadable(err),
    }
}

/// `/status`: a unit command the host answers.
pub(super) fn status(payload: &str) -> Invocation {
    unit(payload, Command::System(SystemCommand::Status))
}

/// `/organizememory`: a unit command the agent is handed.
pub(super) fn organize_memory(payload: &str) -> Invocation {
    unit(payload, Command::Agent(AgentCommand::OrganizeMemory))
}

/// `/lang`'s argument: the language to set, or the reset that drops the chat's own choice.
pub(super) fn lang(payload: &str) -> Invocation {
    let flags = match parse::<LangFlag>(payload, &Grammar::Flags) {
        Ok(flags) => flags,
        Err(err) => return Invocation::Unreadable(err),
    };
    let code = flags.value(LangFlag::Code);
    if flags.given(LangFlag::Reset) && code.is_some() {
        return Invocation::Unreadable(ArgError::Conflict(
            LangFlag::Code.name(),
            LangFlag::Reset.name(),
        ));
    }
    let arg = match code {
        // `/lang reset` names the same reset as a bare word.
        Some("reset") => LanguageArg::Reset,
        Some(code) => match code.parse::<Language>() {
            Ok(language) => LanguageArg::Set(language),
            Err(_) => LanguageArg::Unsupported,
        },
        None if flags.given(LangFlag::Reset) => LanguageArg::Reset,
        None => LanguageArg::Current,
    };
    Invocation::Ready(Command::System(SystemCommand::Lang(arg)))
}

/// `/explain`'s argument: the payload verbatim, and none when nothing was written.
pub(super) fn explain(payload: &str) -> Invocation {
    match parse::<NoFlags>(payload, &Grammar::RawText) {
        Ok(payload) => Invocation::Ready(Command::Agent(AgentCommand::Explain(
            payload
                .text()
                .filter(|target| !target.is_empty())
                .map(str::to_string),
        ))),
        Err(err) => Invocation::Unreadable(err),
    }
}

/// `/purge`'s argument: a stretch, and optionally the surfaces it holds. A bare token fills the first
/// flag still unset, in declaration order (`ALL`), so `/purge all agent,files` names both; without a
/// stretch the dialog asks for one.
pub(super) fn purge(payload: &str) -> Invocation {
    let flags = match parse::<PurgeFlag>(payload, &Grammar::Flags) {
        Ok(flags) => flags,
        Err(err) => return Invocation::Unreadable(err),
    };
    let range = match flags.value(PurgeFlag::Range) {
        None => None,
        Some(token) => match stretch(token) {
            Some(range) => Some(range),
            None => return Invocation::Unreadable(unreadable(PurgeFlag::Range, token)),
        },
    };
    let surfaces = match flags.value(PurgeFlag::Surfaces) {
        None => PurgeSurfaces::all(),
        Some(token) => match surfaces(token) {
            Some(surfaces) => surfaces,
            None => return Invocation::Unreadable(unreadable(PurgeFlag::Surfaces, token)),
        },
    };
    Invocation::Ready(Command::System(SystemCommand::Purge(PurgeArg {
        range,
        surfaces,
    })))
}

/// `/digest`'s argument: the stretch it runs over, absent while nobody has chosen one, and the
/// language it writes the report in, absent while the chat's own stands.
pub(super) fn digest(payload: &str) -> Invocation {
    let flags = match parse::<DigestFlag>(payload, &Grammar::Flags) {
        Ok(flags) => flags,
        Err(err) => return Invocation::Unreadable(err),
    };
    let stretch = match flags.value(DigestFlag::Days) {
        None => None,
        Some(token) => match digest_days(token) {
            Ok(stretch) => Some(stretch),
            Err(err) => return Invocation::Unreadable(err),
        },
    };
    let language = match flags.value(DigestFlag::Lang) {
        None => None,
        Some(token) => match token.parse::<Language>() {
            Ok(language) => Some(language),
            Err(_) => {
                return Invocation::Unreadable(ArgError::BadValue {
                    arg: DigestFlag::Lang.name(),
                    value: token.to_string(),
                });
            }
        },
    };
    Invocation::Ready(Command::System(SystemCommand::Digest(DigestArg {
        stretch,
        language,
    })))
}

/// The stretch a day count names: at least one day, at most [`MAX_DIGEST_DAYS`] of them.
fn digest_days(token: &str) -> Result<Stretch, ArgError> {
    let days = match token.parse::<u32>() {
        Ok(days) if days >= MIN_DIGEST_DAYS => days,
        _ => return Err(unreadable(DigestFlag::Days, token)),
    };
    if days > MAX_DIGEST_DAYS {
        return Err(ArgError::OutOfRange {
            arg: DigestFlag::Days.name(),
            value: token.to_string(),
            max: MAX_DIGEST_DAYS,
        });
    }
    Ok(Stretch::Last {
        secs: i64::from(days) * 86_400,
    })
}

/// A stretch as a person writes it: `all`, a whole number of hours or days back from now, or two
/// local dates covering whole days with the last one included.
fn stretch(token: &str) -> Option<Stretch> {
    if token == "all" {
        return Some(Stretch::All);
    }
    if let Some((count, unit)) = token.split_at_checked(token.len().checked_sub(1)?) {
        let secs = match unit {
            "h" => 3_600,
            "d" => 86_400,
            _ => 0,
        };
        if secs != 0 {
            return count
                .parse::<u32>()
                .ok()
                .filter(|count| *count >= 1)
                .and_then(|count| i64::from(count).checked_mul(secs))
                .map(|secs| Stretch::Last { secs });
        }
    }

    let (from, to) = token.split_once("..")?;
    let zone = jiff::tz::TimeZone::system();
    let from: jiff::civil::Date = from.parse().ok()?;
    let to: jiff::civil::Date = to.parse().ok()?;
    if to < from {
        return None;
    }
    let since = from.to_zoned(zone.clone()).ok()?.timestamp();
    // The statement's upper bound is inclusive, so the last day runs to the nanosecond before the
    // next one starts.
    let until =
        to.tomorrow().ok()?.to_zoned(zone).ok()?.timestamp() - jiff::SignedDuration::from_nanos(1);
    Some(Stretch::Between {
        from: since,
        to: until,
    })
}

/// The surfaces as a comma-separated list of names; an unknown name makes the whole list unreadable,
/// and a name that is not given is off.
fn surfaces(token: &str) -> Option<PurgeSurfaces> {
    let mut surfaces = PurgeSurfaces::none();
    for name in token.split(',') {
        let surface = PurgeSurface::ALL
            .into_iter()
            .find(|surface| surface.name() == name)?;
        surfaces = surfaces.with(surface);
    }
    Some(surfaces)
}

/// A flag's value its own parser refused, as the person wrote it.
fn unreadable(flag: impl FlagName, value: &str) -> ArgError {
    ArgError::BadValue {
        arg: flag.name(),
        value: value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::command::spec::Declaration;

    /// A declared command, by its wire name.
    fn declared(name: &str) -> &'static Declaration {
        crate::agent::command::find(name).expect("a declared command")
    }

    /// The invocation a payload denotes for the command declared under `name`.
    fn parsed(name: &str, payload: &str) -> Invocation {
        (declared(name).parse)(payload)
    }

    #[test]
    fn parse_reads_each_payload() {
        use Command::{Agent, System};
        use LanguageArg::{Current, Reset, Set, Unsupported};
        use SystemCommand::Lang;

        let ready = Invocation::Ready;

        assert_eq!(
            parsed("status", "x"),
            Invocation::Unreadable(ArgError::ExtraPositional("x".into()))
        );
        assert_eq!(
            parsed("organizememory", "x"),
            Invocation::Unreadable(ArgError::ExtraPositional("x".into()))
        );
        assert_eq!(
            parsed("organizememory", ""),
            ready(Agent(AgentCommand::OrganizeMemory))
        );
        assert_eq!(parsed("status", ""), ready(System(SystemCommand::Status)));
        assert_eq!(parsed("lang", "  "), ready(System(Lang(Current))));
        assert_eq!(
            parsed("explain", ""),
            ready(Agent(AgentCommand::Explain(None)))
        );
        assert_eq!(
            parsed("explain", "a  b"),
            ready(Agent(AgentCommand::Explain(Some("a  b".into()))))
        );
        assert_eq!(
            parsed("digest", ""),
            ready(System(SystemCommand::Digest(DigestArg {
                stretch: None,
                language: None,
            }))),
            "a bare `/digest` asks for a stretch in the dialog"
        );
        assert_eq!(
            parsed("digest", "003"),
            ready(System(SystemCommand::Digest(DigestArg {
                stretch: Some(Stretch::Last { secs: 3 * 86_400 }),
                language: None,
            })))
        );
        assert_eq!(
            parsed("digest", "--lang en"),
            ready(System(SystemCommand::Digest(DigestArg {
                stretch: None,
                language: Some(Language::En),
            }))),
            "a language on its own still asks for the stretch in the dialog"
        );
        assert_eq!(
            parsed("digest", "7 --lang=JA"),
            ready(System(SystemCommand::Digest(DigestArg {
                stretch: Some(Stretch::Last { secs: 7 * 86_400 }),
                language: Some(Language::Ja),
            }))),
            "the code is read case-insensitively"
        );
        assert_eq!(
            parsed("digest", "en"),
            Invocation::Unreadable(ArgError::BadValue {
                arg: "days",
                value: "en".into(),
            }),
            "a bare word fills the day count, so a language has to be named"
        );

        // The bare word and the flag naming it are one argument, for every command.
        assert_eq!(parsed("lang", "reset"), parsed("lang", "--reset"));
        assert_eq!(parsed("lang", "--reset"), ready(System(Lang(Reset))));
        assert_eq!(parsed("lang", "--code reset"), ready(System(Lang(Reset))));
        for spelling in ["JA", "--code JA", "--code=JA"] {
            assert_eq!(
                parsed("lang", spelling),
                ready(System(Lang(Set(Language::Ja)))),
                "{spelling}"
            );
        }
        assert_eq!(
            parsed("lang", "EN-GB"),
            ready(System(Lang(Set(Language::EnGb))))
        );
        assert_eq!(parsed("lang", "jp"), ready(System(Lang(Unsupported))));
        assert_eq!(
            parsed("explain", "--verbose is broken"),
            ready(Agent(AgentCommand::Explain(Some(
                "--verbose is broken".into()
            )))),
            "the payload of `/explain` is text, dashes and all"
        );
        for spelling in ["--days 3", "--days=3"] {
            assert_eq!(
                parsed("digest", spelling),
                parsed("digest", "3"),
                "{spelling}"
            );
        }
    }

    /// Every refusal says which argument it refused and why.
    #[test]
    fn parse_says_why_a_payload_is_unreadable() {
        let unreadable = Invocation::Unreadable;
        let cases = [
            (
                declared("digest"),
                "--days abc",
                ArgError::BadValue {
                    arg: "days",
                    value: "abc".into(),
                },
            ),
            (
                declared("digest"),
                "0",
                ArgError::BadValue {
                    arg: "days",
                    value: "0".into(),
                },
            ),
            (declared("digest"), "--days", ArgError::MissingValue("days")),
            (
                declared("digest"),
                "--days 1 --days 2",
                ArgError::Duplicate("days"),
            ),
            (
                declared("digest"),
                "--bogus 1",
                ArgError::UnknownFlag("--bogus".into()),
            ),
            (
                declared("digest"),
                "366",
                ArgError::OutOfRange {
                    arg: "days",
                    value: "366".into(),
                    max: 365,
                },
            ),
            (
                declared("digest"),
                "--lang zz",
                ArgError::BadValue {
                    arg: "lang",
                    value: "zz".into(),
                },
            ),
            (declared("digest"), "--lang", ArgError::MissingValue("lang")),
            (
                declared("lang"),
                "--reset=1",
                ArgError::UnexpectedValue("reset"),
            ),
            (
                declared("purge"),
                "all agent,files extra",
                ArgError::ExtraPositional("extra".into()),
            ),
            (
                declared("lang"),
                "--code zh --reset",
                ArgError::Conflict("code", "reset"),
            ),
        ];
        for (command, payload, reason) in cases {
            assert_eq!(
                (command.parse)(payload),
                unreadable(reason.clone()),
                "/{} {payload}",
                command.name
            );
        }

        assert_eq!(
            parsed("purge", "--surfaces agent"),
            Invocation::Ready(Command::System(SystemCommand::Purge(PurgeArg {
                range: None,
                surfaces: PurgeSurfaces::none().with(PurgeSurface::Agent),
            }))),
            "naming the surfaces as a flag with no stretch goes on to the stretch question"
        );
    }

    /// `/purge`'s grammar: a stretch alone or with the surfaces it holds, and a date range whose last
    /// day is included whole.
    #[test]
    fn purge_reads_its_stretch_and_surfaces() {
        use Command::System;
        use SystemCommand::Purge;

        let ready =
            |range, surfaces| Invocation::Ready(System(Purge(PurgeArg { range, surfaces })));
        let all = PurgeSurfaces::all();

        assert_eq!(parsed("purge", ""), ready(None, all));
        assert_eq!(
            parsed("purge", "   "),
            ready(None, all),
            "a bare command starts at the stretch question"
        );
        assert_eq!(
            parsed("purge", "7d"),
            ready(Some(Stretch::Last { secs: 7 * 86_400 }), all)
        );
        let agent_and_files = PurgeSurfaces::none()
            .with(PurgeSurface::Agent)
            .with(PurgeSurface::Files);
        assert_eq!(
            parsed("purge", "all agent,files"),
            ready(Some(Stretch::All), agent_and_files)
        );
        assert_eq!(
            parsed("purge", "--range all --surfaces=agent,files"),
            ready(Some(Stretch::All), agent_and_files),
            "the flags are the same argument as the bare words"
        );

        for malformed in [
            "0h",
            "7x",
            "7",
            "2026-10-05..2026-10-01",
            "7d agent,bogus",
            "7d agent files",
        ] {
            assert!(
                matches!(parsed("purge", malformed), Invocation::Unreadable(_)),
                "{malformed}"
            );
        }

        let Invocation::Ready(System(Purge(PurgeArg {
            range: Some(Stretch::Between { from, to }),
            ..
        }))) = parsed("purge", "2026-10-01..2026-10-05")
        else {
            panic!("expected a date range");
        };
        let zone = jiff::tz::TimeZone::system();
        let local = |date: jiff::civil::Date| {
            date.to_zoned(zone.clone())
                .expect("a local date")
                .timestamp()
        };
        assert_eq!(
            from,
            local(jiff::civil::Date::constant(2026, 10, 1)),
            "the first day starts at its own midnight"
        );
        assert_eq!(
            to,
            jiff::civil::Date::constant(2026, 10, 5)
                .at(23, 59, 59, 999_999_999)
                .to_zoned(zone)
                .expect("a local time")
                .timestamp(),
            "the last day runs to its own last nanosecond"
        );
    }
}
