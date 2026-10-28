//! What each command's payload denotes: the declared grammar is read, the values checked, and the
//! invocation built.

use super::{
    ArgError, ArgName, Command, ContentPolicyArg, DigestArg, Grammar, Invocation, LanguageArg,
    OptionSpec, Parsed, PersonaArg, Positional, PurgeArg, SystemCommand, args::parse,
    digest::Delivery,
};
use crate::{
    config::conversation::{ContentPolicy, Language},
    domain::{
        service::settings::{RESET_ARG, RESET_FLAG},
        vo::{AgentCommand, PurgeSurface, PurgeSurfaces, Stretch},
    },
};

/// The reset option both settings grammars carry; the spelling lives in one place.
const RESET_OPTIONS: &[OptionSpec] = &[OptionSpec {
    name: RESET_FLAG,
    value: None,
}];

/// A `/digest` day count covers at least one day: digesting none of them is not a request.
pub(crate) const MIN_DIGEST_DAYS: u32 = 1;

/// The most days `/digest` runs over at once.
pub(crate) const MAX_DIGEST_DAYS: u32 = 365;

pub(super) const LANG_GRAMMAR: Grammar = Grammar {
    positionals: &[Positional {
        name: "CODE",
        required: false,
    }],
    options: RESET_OPTIONS,
    raw_tail: false,
};

/// The named level `/content-policy` takes.
pub(super) const LEVEL: &str = "LEVEL";

pub(super) const CONTENT_POLICY_GRAMMAR: Grammar = Grammar {
    positionals: &[Positional {
        name: LEVEL,
        required: false,
    }],
    options: RESET_OPTIONS,
    raw_tail: false,
};

pub(super) const PERSONA_GRAMMAR: Grammar = Grammar::RAW;

/// The range spelling shared by `/purge` and `/digest`.
pub(super) const RANGE: &str = "RANGE";

/// The confirmation `/purge` needs on a channel without a page.
pub(super) const YES: &str = "yes";

pub(super) const PURGE_GRAMMAR: Grammar = Grammar {
    positionals: &[Positional {
        name: RANGE,
        required: false,
    }],
    options: &[
        OptionSpec {
            name: "surfaces",
            value: Some("LIST"),
        },
        OptionSpec {
            name: YES,
            value: None,
        },
    ],
    raw_tail: false,
};

pub(super) const DIGEST_GRAMMAR: Grammar = Grammar {
    positionals: &[Positional {
        name: RANGE,
        required: false,
    }],
    options: &[
        OptionSpec {
            name: "lang",
            value: Some("CODE"),
        },
        OptionSpec {
            name: "to",
            value: Some("here|me"),
        },
    ],
    raw_tail: false,
};

pub(super) const EXPLAIN_GRAMMAR: Grammar = Grammar::RAW;

/// Reads an option/positional payload. Help and a refusal leave as the invocation the host answers;
/// a raw tail belongs to `explain`, which reads its own grammar.
fn read_args<'a>(payload: &'a str, grammar: &Grammar) -> Result<Parsed<'a>, Invocation> {
    match parse(payload, grammar) {
        Ok(Parsed::Help) => Err(Invocation::Help),
        Ok(parsed @ Parsed::Args { .. }) => Ok(parsed),
        Ok(Parsed::Raw(_)) => unreachable!("a grammar without a raw tail never parses one"),
        Err(err) => Err(Invocation::Unreadable(err)),
    }
}

fn unit(payload: &str, grammar: &Grammar, command: Command) -> Invocation {
    match read_args(payload, grammar) {
        Ok(_) => Invocation::Ready(command),
        Err(invocation) => invocation,
    }
}

pub(super) fn status(payload: &str) -> Invocation {
    unit(
        payload,
        &Grammar::NONE,
        Command::System(SystemCommand::Status),
    )
}

pub(super) fn lang(payload: &str) -> Invocation {
    let parsed = match read_args(payload, &LANG_GRAMMAR) {
        Ok(parsed) => parsed,
        Err(invocation) => return invocation,
    };
    let code = parsed.positional(0);
    if parsed.given("reset") {
        if code.is_some() {
            return Invocation::Unreadable(ArgError::Conflict(
                ArgName::Positional("CODE"),
                ArgName::Option("reset"),
            ));
        }
        return Invocation::Ready(Command::System(SystemCommand::Lang(LanguageArg::Reset)));
    }
    let arg = match code {
        Some(code) => match code.parse::<Language>() {
            Ok(language) => LanguageArg::Set(language),
            Err(_) => LanguageArg::Unsupported,
        },
        None => LanguageArg::Current,
    };
    Invocation::Ready(Command::System(SystemCommand::Lang(arg)))
}

pub(super) fn content_policy(payload: &str) -> Invocation {
    let parsed = match read_args(payload, &CONTENT_POLICY_GRAMMAR) {
        Ok(parsed) => parsed,
        Err(invocation) => return invocation,
    };
    if parsed.given("reset") {
        if parsed.positional(0).is_some() {
            return Invocation::Unreadable(ArgError::Conflict(
                ArgName::Positional(LEVEL),
                ArgName::Option("reset"),
            ));
        }
        return Invocation::Ready(Command::System(SystemCommand::ContentPolicy(
            ContentPolicyArg::Reset,
        )));
    }
    let arg = match parsed.positional(0) {
        Some(level) => match level.parse::<ContentPolicy>() {
            Ok(level) => ContentPolicyArg::Set(level),
            Err(_) => {
                return Invocation::Unreadable(ArgError::BadValue {
                    arg: ArgName::Positional(LEVEL),
                    value: level.to_owned(),
                });
            }
        },
        None => ContentPolicyArg::Current,
    };
    Invocation::Ready(Command::System(SystemCommand::ContentPolicy(arg)))
}

pub(super) fn persona(payload: &str) -> Invocation {
    let text = payload.trim();
    if text == "--help" || text == "-h" {
        return Invocation::Help;
    }
    // `--` escapes the whole tail: `persona -- --reset` sets the literal text, not the flag.
    let escaped = text.strip_prefix("-- ");
    let text = escaped.unwrap_or(text);
    if escaped.is_none()
        && (text == RESET_ARG
            || text
                .strip_suffix(RESET_ARG)
                .is_some_and(|prefix| prefix.ends_with(' ')))
    {
        return Invocation::Ready(Command::System(SystemCommand::Persona(PersonaArg::Reset)));
    }
    let text = text.trim();
    let arg = if text.is_empty() || text == "--" {
        PersonaArg::Current
    } else {
        PersonaArg::Set(text.to_owned())
    };
    Invocation::Ready(Command::System(SystemCommand::Persona(arg)))
}

pub(super) fn purge(payload: &str) -> Invocation {
    let parsed = match read_args(payload, &PURGE_GRAMMAR) {
        Ok(parsed) => parsed,
        Err(invocation) => return invocation,
    };
    let range = match parsed.positional(0) {
        None => None,
        Some(token) => match stretch(token) {
            Some(range) => Some(range),
            None => {
                return Invocation::Unreadable(ArgError::BadValue {
                    arg: ArgName::Positional("RANGE"),
                    value: token.to_owned(),
                });
            }
        },
    };
    let surfaces = match parsed.value("surfaces") {
        None => PurgeSurfaces::all(),
        Some(token) => match surfaces(token) {
            Some(surfaces) => surfaces,
            None => {
                return Invocation::Unreadable(ArgError::BadValue {
                    arg: ArgName::Option("surfaces"),
                    value: token.to_owned(),
                });
            }
        },
    };
    let yes = parsed.given("yes");
    if yes && range.is_none() {
        return Invocation::Unreadable(ArgError::MissingArgument(ArgName::Positional("RANGE")));
    }
    Invocation::Ready(Command::System(SystemCommand::Purge(PurgeArg {
        range,
        surfaces,
        yes,
    })))
}

pub(super) fn digest(payload: &str) -> Invocation {
    let parsed = match read_args(payload, &DIGEST_GRAMMAR) {
        Ok(parsed) => parsed,
        Err(invocation) => return invocation,
    };
    let stretch = match parsed.positional(0) {
        None => None,
        Some(token) => match digest_stretch(token) {
            Ok(stretch) => Some(stretch),
            Err(err) => return Invocation::Unreadable(err),
        },
    };
    let language = match parsed.value("lang") {
        None => None,
        Some(token) => match token.parse::<Language>() {
            Ok(language) => Some(language),
            Err(_) => {
                return Invocation::Unreadable(ArgError::BadValue {
                    arg: ArgName::Option("lang"),
                    value: token.to_owned(),
                });
            }
        },
    };
    let delivery = match parsed.value("to") {
        None => None,
        Some("here") => Some(Delivery::Chat),
        Some("me") => Some(Delivery::Person),
        Some(token) => {
            return Invocation::Unreadable(ArgError::BadValue {
                arg: ArgName::Option("to"),
                value: token.to_owned(),
            });
        }
    };
    Invocation::Ready(Command::System(SystemCommand::Digest(DigestArg {
        stretch,
        language,
        delivery,
    })))
}

/// The payload verbatim; `None` when nothing was written.
pub(super) fn explain(payload: &str) -> Invocation {
    match parse(payload, &EXPLAIN_GRAMMAR) {
        Ok(Parsed::Help) => Invocation::Help,
        Ok(Parsed::Raw(text)) => Invocation::Ready(Command::Agent(AgentCommand::Explain(
            (!text.is_empty()).then(|| text.to_owned()),
        ))),
        Ok(Parsed::Args { .. }) => unreachable!("the explain grammar has no options"),
        Err(err) => Invocation::Unreadable(err),
    }
}

/// A digest stretch is a whole number of days inside the report's own window; anything else is
/// refused with the bound it missed.
fn digest_stretch(token: &str) -> Result<Stretch, ArgError> {
    let stretch = stretch(token).ok_or_else(|| ArgError::BadValue {
        arg: ArgName::Positional("RANGE"),
        value: token.to_owned(),
    })?;
    let days = match stretch {
        Stretch::All => {
            return Err(ArgError::BadValue {
                arg: ArgName::Positional("RANGE"),
                value: token.to_owned(),
            });
        }
        Stretch::Last { secs } if secs % 86_400 == 0 => secs / 86_400,
        Stretch::Last { .. } => {
            return Err(ArgError::BadValue {
                arg: ArgName::Positional("RANGE"),
                value: token.to_owned(),
            });
        }
        Stretch::Between { from, to } => (to.as_second() - from.as_second()).div_euclid(86_400) + 1,
    };
    if !(i64::from(MIN_DIGEST_DAYS)..=i64::from(MAX_DIGEST_DAYS)).contains(&days) {
        return Err(ArgError::OutOfRange {
            arg: ArgName::Positional("RANGE"),
            value: token.to_owned(),
            min: MIN_DIGEST_DAYS,
            max: MAX_DIGEST_DAYS,
        });
    }
    Ok(stretch)
}

/// Reads the shared `RANGE` spelling documented in `docs/topics/commands.md`.
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
    let from = jiff::civil::Date::strptime("%Y-%m-%d", from)
        .ok()?
        .to_zoned(zone.clone())
        .ok()?
        .timestamp();
    let to = jiff::civil::Date::strptime("%Y-%m-%d", to)
        .ok()?
        .tomorrow()
        .ok()?
        .to_zoned(zone)
        .ok()?
        .timestamp()
        - jiff::SignedDuration::from_nanos(1);
    (from <= to).then_some(Stretch::Between { from, to })
}

/// The four surfaces a comma-separated token names, or none when one of them is not a surface.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The invocation a payload denotes for the command declared under `name`.
    fn parsed(name: &str, payload: &str) -> Invocation {
        let declared = crate::agent::command::find(name, crate::agent::command::CommandGates::ALL)
            .expect("a declared command");
        (declared.parse)(payload)
    }

    #[test]
    fn parse_reads_each_payload() {
        use Command::{Agent, System};
        use LanguageArg::{Current, Reset, Set, Unsupported};
        use PersonaArg::{Reset as PersonaReset, Set as PersonaSet};
        use SystemCommand::{Lang, Persona};

        let ready = Invocation::Ready;

        assert_eq!(
            parsed("status", "x"),
            Invocation::Unreadable(ArgError::ExtraPositional("x".into()))
        );
        assert_eq!(parsed("status", ""), ready(System(SystemCommand::Status)));
        assert_eq!(parsed("lang", "  "), ready(System(Lang(Current))));

        for spelling in ["JA", "ja", "EN-GB"] {
            let expected = if spelling == "EN-GB" {
                Language::EnGb
            } else {
                Language::Ja
            };
            assert_eq!(
                parsed("lang", spelling),
                ready(System(Lang(Set(expected)))),
                "{spelling}"
            );
        }
        assert_eq!(parsed("lang", "jp"), ready(System(Lang(Unsupported))));
        assert_eq!(
            parsed("lang", "reset"),
            ready(System(Lang(Unsupported))),
            "reset is a value, not a reset word"
        );
        assert_eq!(parsed("lang", "--reset"), ready(System(Lang(Reset))));
        assert_eq!(
            parsed("lang", "zh --reset"),
            Invocation::Unreadable(ArgError::Conflict(
                ArgName::Positional("CODE"),
                ArgName::Option("reset")
            ))
        );

        let policy = |arg| System(SystemCommand::ContentPolicy(arg));
        assert_eq!(
            parsed("content-policy", ""),
            ready(policy(ContentPolicyArg::Current))
        );
        for level in ContentPolicy::ALL {
            assert_eq!(
                parsed("content-policy", level.as_str()),
                ready(policy(ContentPolicyArg::Set(level))),
                "{}",
                level.as_str()
            );
        }
        assert_eq!(
            parsed("content-policy", "--reset"),
            ready(policy(ContentPolicyArg::Reset))
        );
        assert_eq!(
            parsed("content-policy", "anything"),
            Invocation::Unreadable(ArgError::BadValue {
                arg: ArgName::Positional(LEVEL),
                value: "anything".into(),
            }),
            "an unknown level is a parse refusal, not a handler case"
        );
        assert_eq!(
            parsed("content-policy", "safe --reset"),
            Invocation::Unreadable(ArgError::Conflict(
                ArgName::Positional(LEVEL),
                ArgName::Option("reset")
            ))
        );

        assert_eq!(
            parsed("persona", "--reset"),
            ready(System(Persona(PersonaReset)))
        );
        assert_eq!(
            parsed("persona", "-- --reset"),
            ready(System(Persona(PersonaSet("--reset".into()))))
        );

        assert_eq!(
            parsed("explain", ""),
            ready(Agent(AgentCommand::Explain(None)))
        );
        assert_eq!(
            parsed("explain", "a  b"),
            ready(Agent(AgentCommand::Explain(Some("a  b".into())))),
            "the raw tail keeps its spacing"
        );
        assert_eq!(
            parsed("explain", "-- --verbose is broken"),
            ready(Agent(AgentCommand::Explain(Some(
                "--verbose is broken".into()
            )))),
            "a payload that starts with a dash is written after `--`"
        );

        assert_eq!(
            parsed("digest", ""),
            ready(System(SystemCommand::Digest(DigestArg {
                stretch: None,
                language: None,
                delivery: None,
            }))),
            "a bare `/digest` asks for a stretch in the dialog"
        );
        assert_eq!(
            parsed("digest", "7d"),
            ready(System(SystemCommand::Digest(DigestArg {
                stretch: Some(Stretch::Last { secs: 7 * 86_400 }),
                language: None,
                delivery: None,
            })))
        );
        assert_eq!(
            parsed("digest", "7d --lang ja --to me"),
            ready(System(SystemCommand::Digest(DigestArg {
                stretch: Some(Stretch::Last { secs: 7 * 86_400 }),
                language: Some(Language::Ja),
                delivery: Some(Delivery::Person),
            })))
        );
        assert_eq!(
            parsed("digest", "7d --to=here"),
            ready(System(SystemCommand::Digest(DigestArg {
                stretch: Some(Stretch::Last { secs: 7 * 86_400 }),
                language: None,
                delivery: Some(Delivery::Chat),
            })))
        );
        assert_eq!(
            parsed("digest", "en"),
            Invocation::Unreadable(ArgError::BadValue {
                arg: ArgName::Positional("RANGE"),
                value: "en".into(),
            }),
            "the positional is the range; a language is always named"
        );
    }

    /// Every refusal says which argument it refused and why.
    #[test]
    fn parse_says_why_a_payload_is_unreadable() {
        let unreadable = Invocation::Unreadable;
        let cases = [
            (
                "digest",
                "abc",
                ArgError::BadValue {
                    arg: ArgName::Positional("RANGE"),
                    value: "abc".into(),
                },
            ),
            (
                "digest",
                "0d",
                ArgError::BadValue {
                    arg: ArgName::Positional("RANGE"),
                    value: "0d".into(),
                },
            ),
            (
                "digest",
                "25h",
                ArgError::BadValue {
                    arg: ArgName::Positional("RANGE"),
                    value: "25h".into(),
                },
            ),
            (
                "digest",
                "366d",
                ArgError::OutOfRange {
                    arg: ArgName::Positional("RANGE"),
                    value: "366d".into(),
                    min: 1,
                    max: 365,
                },
            ),
            (
                "digest",
                "7d --lang",
                ArgError::MissingValue(ArgName::Option("lang")),
            ),
            (
                "digest",
                "7d --lang zz",
                ArgError::BadValue {
                    arg: ArgName::Option("lang"),
                    value: "zz".into(),
                },
            ),
            (
                "digest",
                "7d --to there",
                ArgError::BadValue {
                    arg: ArgName::Option("to"),
                    value: "there".into(),
                },
            ),
            (
                "digest",
                "7d --lang en --lang zh",
                ArgError::Duplicate(ArgName::Option("lang")),
            ),
            (
                "digest",
                "--bogus 1",
                ArgError::UnknownOption("--bogus".into()),
            ),
            (
                "lang",
                "--reset=1",
                ArgError::UnexpectedValue(ArgName::Option("reset")),
            ),
            (
                "purge",
                "--yes",
                ArgError::MissingArgument(ArgName::Positional("RANGE")),
            ),
            (
                "purge",
                "7d agent,files",
                ArgError::ExtraPositional("agent,files".into()),
            ),
            (
                "purge",
                "7d --surfaces agent,bogus",
                ArgError::BadValue {
                    arg: ArgName::Option("surfaces"),
                    value: "agent,bogus".into(),
                },
            ),
        ];
        for (name, payload, reason) in cases {
            assert_eq!(
                parsed(name, payload),
                unreadable(reason.clone()),
                "/{name} {payload}"
            );
        }
    }

    /// `/purge`'s grammar: a stretch alone or with the surfaces it holds, and a date range whose last
    /// day is included whole.
    #[test]
    fn purge_reads_its_stretch_and_surfaces() {
        use Command::System;
        use SystemCommand::Purge;

        let ready = |range, surfaces, yes| {
            Invocation::Ready(System(Purge(PurgeArg {
                range,
                surfaces,
                yes,
            })))
        };
        let all = PurgeSurfaces::all();

        assert_eq!(parsed("purge", ""), ready(None, all, false));
        assert_eq!(
            parsed("purge", "   "),
            ready(None, all, false),
            "a bare command starts at the stretch question"
        );
        assert_eq!(
            parsed("purge", "7d"),
            ready(Some(Stretch::Last { secs: 7 * 86_400 }), all, false)
        );
        assert_eq!(
            parsed("purge", "7d --yes"),
            ready(Some(Stretch::Last { secs: 7 * 86_400 }), all, true),
            "the command line carries its own confirmation"
        );
        let agent_and_files = PurgeSurfaces::none()
            .with(PurgeSurface::Agent)
            .with(PurgeSurface::Files);
        assert_eq!(
            parsed("purge", "all --surfaces agent,files --yes"),
            ready(Some(Stretch::All), agent_and_files, true)
        );
        assert_eq!(
            parsed("purge", "--surfaces=agent,files"),
            ready(None, agent_and_files, false),
            "naming the surfaces with no stretch goes on to the stretch question"
        );

        for malformed in ["0h", "7x", "7", "2026-10-05..2026-10-01", "7d agent files"] {
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

    /// Every command answers `--help` with the same synopsis the declaration spells.
    #[test]
    fn every_command_answers_help() {
        for name in [
            "status",
            "lang",
            "content-policy",
            "persona",
            "purge",
            "explain",
            "digest",
        ] {
            assert_eq!(parsed(name, "--help"), Invocation::Help, "{name}");
            assert_eq!(parsed(name, "-h"), Invocation::Help, "{name}");
        }
    }
}
