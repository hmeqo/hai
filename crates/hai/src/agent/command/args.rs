//! The one argument grammar every command shares. A command declares its positionals and long
//! options once; the same declaration reads the payload and spells the synopsis a person is shown.
//! The rules live in `docs/topics/commands.md`.

use std::fmt::Write;

/// One positional a command declares, in the order a person writes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Positional {
    pub(crate) name: &'static str,
    pub(crate) required: bool,
}

/// One long option a command declares. `value` is the placeholder its value is written under, and
/// `None` marks a flag that takes no value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OptionSpec {
    pub(crate) name: &'static str,
    pub(crate) value: Option<&'static str>,
}

/// What one command's payload may contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Grammar {
    pub(crate) positionals: &'static [Positional],
    pub(crate) options: &'static [OptionSpec],
    /// The first free token starts a verbatim tail (`explain`): everything from there on is text.
    pub(crate) raw_tail: bool,
}

impl Grammar {
    pub(crate) const NONE: Self = Self {
        positionals: &[],
        options: &[],
        raw_tail: false,
    };

    pub(crate) const RAW: Self = Self {
        positionals: &[],
        options: &[],
        raw_tail: true,
    };

    /// The form a person types, spelled from this declaration.
    pub(crate) fn synopsis(&self, name: &str) -> String {
        let mut text = name.to_owned();
        for positional in self.positionals {
            let _ = if positional.required {
                write!(text, " {}", positional.name)
            } else {
                write!(text, " [{}]", positional.name)
            };
        }
        for option in self.options {
            let _ = match option.value {
                Some(value) => write!(text, " [--{} {}]", option.name, value),
                None => write!(text, " [--{}]", option.name),
            };
        }
        if self.raw_tail {
            text.push_str(" [--] [TEXT...]");
        }
        text
    }
}

/// A payload the grammar has read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Parsed<'a> {
    Args {
        positionals: Vec<&'a str>,
        options: Vec<(&'static str, Option<&'a str>)>,
    },
    Raw(&'a str),
    Help,
}

impl<'a> Parsed<'a> {
    pub(crate) fn positional(&self, index: usize) -> Option<&'a str> {
        match self {
            Self::Args { positionals, .. } => positionals.get(index).copied(),
            Self::Raw(_) | Self::Help => None,
        }
    }

    /// The value `name` carried; `None` when it was not written or is a flag without one.
    pub(crate) fn value(&self, name: &str) -> Option<&'a str> {
        match self {
            Self::Args { options, .. } => options
                .iter()
                .find(|(written, _)| *written == name)
                .and_then(|(_, value)| *value),
            Self::Raw(_) | Self::Help => None,
        }
    }

    /// Whether `name` was written at all. A flag is read by presence, never by emptiness.
    pub(crate) fn given(&self, name: &str) -> bool {
        match self {
            Self::Args { options, .. } => options.iter().any(|(written, _)| *written == name),
            Self::Raw(_) | Self::Help => false,
        }
    }
}

/// Reads `payload` the way `grammar` declares. The payload is trimmed at its ends; whitespace inside
/// a raw tail is kept as written, and no quoting is invented.
pub(crate) fn parse<'a>(payload: &'a str, grammar: &Grammar) -> Result<Parsed<'a>, ArgError> {
    let tokens = tokens(payload);
    let mut positionals: Vec<&str> = Vec::new();
    let mut options: Vec<(&'static str, Option<&str>)> = Vec::new();
    let mut literal = false;
    let mut index = 0;

    while index < tokens.len() {
        let (start, token) = tokens[index];
        index += 1;

        if !literal && (token == "--help" || token == "-h") {
            return Ok(Parsed::Help);
        }

        if !literal && token == "--" {
            if grammar.raw_tail {
                let rest = tokens
                    .get(index)
                    .map_or(&payload[payload.len()..], |(start, _)| &payload[*start..]);
                return Ok(Parsed::Raw(rest.trim_end()));
            }
            literal = true;
            continue;
        }

        if grammar.raw_tail && !literal && (!token.starts_with('-') || token == "-") {
            return Ok(Parsed::Raw(payload[start..].trim_end()));
        }

        if !literal && token.starts_with("--") {
            let (name, inline) = split_option(token);
            let Some(spec) = grammar.options.iter().find(|option| option.name == name) else {
                return Err(ArgError::UnknownOption(token.to_owned()));
            };
            if options.iter().any(|(written, _)| *written == spec.name) {
                return Err(ArgError::Duplicate(ArgName::Option(spec.name)));
            }
            let value = match (spec.value, inline) {
                (None, Some(_)) => {
                    return Err(ArgError::UnexpectedValue(ArgName::Option(spec.name)));
                }
                (None, None) => None,
                (Some(_), Some(value)) => Some(value),
                (Some(_), None) => {
                    let Some((_, value)) = tokens.get(index) else {
                        return Err(ArgError::MissingValue(ArgName::Option(spec.name)));
                    };
                    if value.starts_with("--") || *value == "-h" {
                        return Err(ArgError::MissingValue(ArgName::Option(spec.name)));
                    }
                    index += 1;
                    Some(*value)
                }
            };
            options.push((spec.name, value));
            continue;
        }

        if !literal && token.starts_with('-') && token != "-" {
            return Err(ArgError::UnknownOption(token.to_owned()));
        }

        positionals.push(token);
    }

    if grammar.raw_tail {
        return Ok(Parsed::Raw(""));
    }

    if positionals.len() > grammar.positionals.len() {
        return Err(ArgError::ExtraPositional(
            positionals[grammar.positionals.len()].to_owned(),
        ));
    }
    if let Some(missing) = grammar
        .positionals
        .iter()
        .skip(positionals.len())
        .find(|positional| positional.required)
    {
        return Err(ArgError::MissingArgument(ArgName::Positional(missing.name)));
    }

    Ok(Parsed::Args {
        positionals,
        options,
    })
}

/// The tokens of a payload, each with the byte offset it starts at, so a raw tail can be sliced out
/// of the original text instead of being rebuilt.
fn tokens(input: &str) -> Vec<(usize, &str)> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (at, ch) in input.char_indices() {
        if ch.is_whitespace() {
            if let Some(from) = start.take() {
                tokens.push((from, &input[from..at]));
            }
        } else if start.is_none() {
            start = Some(at);
        }
    }
    if let Some(from) = start {
        tokens.push((from, &input[from..]));
    }
    tokens
}

/// Splits one `--name` or `--name=value` token into the option it names and the value written
/// inline. The caller has already established that the token starts with `--`.
fn split_option(token: &str) -> (&str, Option<&str>) {
    let name = &token[2..];
    match name.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (name, None),
    }
}

/// How an argument is named in a refusal: an option keeps its dashes, a positional is written as its
/// placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArgName {
    Option(&'static str),
    Positional(&'static str),
}

impl ArgName {
    /// The spelling a person wrote, or the placeholder they were meant to write.
    pub(crate) fn render(self) -> String {
        match self {
            Self::Option(name) => format!("--{name}"),
            Self::Positional(name) => name.to_owned(),
        }
    }
}

/// Why a payload is not a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ArgError {
    /// The token as written, dashes included.
    UnknownOption(String),
    MissingValue(ArgName),
    UnexpectedValue(ArgName),
    Duplicate(ArgName),
    Conflict(ArgName, ArgName),
    ExtraPositional(String),
    /// The command needs an argument the payload did not carry; unlike a missing value, the argument
    /// itself is absent.
    MissingArgument(ArgName),
    /// A value the argument's own parser refused; `value` is what the person wrote.
    BadValue {
        arg: ArgName,
        value: String,
    },
    OutOfRange {
        arg: ArgName,
        value: String,
        min: u32,
        max: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRAMMAR: Grammar = Grammar {
        positionals: &[Positional {
            name: "RANGE",
            required: false,
        }],
        options: &[
            OptionSpec {
                name: "surfaces",
                value: Some("LIST"),
            },
            OptionSpec {
                name: "yes",
                value: None,
            },
        ],
        raw_tail: false,
    };

    fn parsed(payload: &str) -> Parsed<'_> {
        parse(payload, &GRAMMAR).expect("readable")
    }

    /// The two spellings of one option are one argument, and the positional is the one declared.
    #[test]
    fn options_and_positionals_are_read_by_their_declaration() {
        for payload in [
            "--surfaces agent,files 7d",
            "7d --surfaces=agent,files",
            "7d --surfaces agent,files",
        ] {
            let read = parsed(payload);
            assert_eq!(read.positional(0), Some("7d"), "{payload}");
            assert_eq!(read.value("surfaces"), Some("agent,files"), "{payload}");
        }
        assert!(parsed("--yes").given("yes"));
        assert_eq!(parsed("--yes").value("yes"), None);
    }

    /// A bare token is a declared positional and nothing else, so a second one is extra.
    #[test]
    fn a_bare_token_is_only_the_declared_positional() {
        assert_eq!(
            parse("agent,files", &GRAMMAR),
            Ok(Parsed::Args {
                positionals: vec!["agent,files"],
                options: Vec::new(),
            })
        );
        assert_eq!(
            parse("7d agent,files", &GRAMMAR),
            Err(ArgError::ExtraPositional("agent,files".to_owned()))
        );
    }

    /// `--` ends options: the rest is positional, even when it starts with a dash.
    #[test]
    fn the_terminator_ends_option_parsing() {
        let read = parsed("-- --yes");
        assert_eq!(read.positional(0), Some("--yes"));
        assert!(!read.given("yes"));
    }

    /// `-h` and `--help` are universal; a single dash is a positional.
    #[test]
    fn help_is_universal_and_a_lone_dash_is_a_positional() {
        assert_eq!(parse("-h", &GRAMMAR), Ok(Parsed::Help));
        assert_eq!(parse("--help", &GRAMMAR), Ok(Parsed::Help));
        assert_eq!(parsed("-").positional(0), Some("-"));
    }

    /// A raw tail starts at its first free token and is kept verbatim, dashes and spacing included;
    /// `--` is how text that starts with a dash is written.
    #[test]
    fn a_raw_tail_is_verbatim() {
        assert_eq!(parse("a  b", &Grammar::RAW), Ok(Parsed::Raw("a  b")));
        assert_eq!(parse("-", &Grammar::RAW), Ok(Parsed::Raw("-")));
        assert_eq!(parse("-- --help", &Grammar::RAW), Ok(Parsed::Raw("--help")));
        assert_eq!(parse("--", &Grammar::RAW), Ok(Parsed::Raw("")));
        assert_eq!(parse("", &Grammar::RAW), Ok(Parsed::Raw("")));
    }

    /// Every refusal names what it refused and why.
    #[test]
    fn every_refusal_names_its_reason() {
        let cases = [
            ("--bogus 1", ArgError::UnknownOption("--bogus".to_owned())),
            ("-x", ArgError::UnknownOption("-x".to_owned())),
            (
                "--surfaces",
                ArgError::MissingValue(ArgName::Option("surfaces")),
            ),
            (
                "--surfaces --yes",
                ArgError::MissingValue(ArgName::Option("surfaces")),
            ),
            ("--yes=1", ArgError::UnexpectedValue(ArgName::Option("yes"))),
            ("--yes --yes", ArgError::Duplicate(ArgName::Option("yes"))),
        ];
        for (payload, expected) in cases {
            assert_eq!(parse(payload, &GRAMMAR), Err(expected), "{payload}");
        }
    }

    /// The synopsis is the declaration, not a second hand-written line.
    #[test]
    fn the_synopsis_follows_the_declaration() {
        assert_eq!(
            GRAMMAR.synopsis("purge"),
            "purge [RANGE] [--surfaces LIST] [--yes]"
        );
        assert_eq!(Grammar::NONE.synopsis("status"), "status");
        assert_eq!(Grammar::RAW.synopsis("explain"), "explain [--] [TEXT...]");
        const REQUIRED: Grammar = Grammar {
            positionals: &[Positional {
                name: "CODE",
                required: true,
            }],
            options: &[],
            raw_tail: false,
        };
        assert_eq!(REQUIRED.synopsis("lang"), "lang CODE");
        assert_eq!(
            parse("", &REQUIRED),
            Err(ArgError::MissingArgument(ArgName::Positional("CODE")))
        );
    }
}
