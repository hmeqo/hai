//! The argument grammar every command shares: how a payload is read, and the reasons one cannot be.

use std::collections::HashMap;

/// One flag a command declares.
pub(crate) trait FlagName: Copy + Eq + std::hash::Hash + 'static {
    /// Every flag of the command.
    const ALL: &'static [Self];

    /// The spelling a person writes after `--`, without the dashes.
    fn name(self) -> &'static str;

    /// Whether a person writes it with a value (`--days 7`); a flag that takes none is written bare
    /// (`--reset`).
    fn takes_value(self) -> bool;
}

/// The flag set of a command that has none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum NoFlags {}

impl FlagName for NoFlags {
    const ALL: &'static [Self] = &[];

    fn name(self) -> &'static str {
        match self {}
    }

    fn takes_value(self) -> bool {
        match self {}
    }
}

/// How a command reads its payload.
pub(crate) enum Grammar {
    /// No payload at all.
    None,
    /// Flags; a bare token fills the first flag that takes a value and is still unset, in `ALL`
    /// order.
    Flags,
    /// The whole payload verbatim, as one free-text argument.
    RawText,
}

/// A payload its command's grammar has read.
#[derive(Debug, Clone)]
pub(crate) enum Payload<F> {
    None,
    Flags(HashMap<F, Option<String>>),
    RawText(String),
}

impl<F: FlagName> PartialEq for Payload<F> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::None, Self::None) => true,
            (Self::Flags(self_flags), Self::Flags(other_flags)) => self_flags == other_flags,
            (Self::RawText(self_text), Self::RawText(other_text)) => self_text == other_text,
            _ => false,
        }
    }
}

impl<F: FlagName> Eq for Payload<F> {}

impl<F: FlagName> Payload<F> {
    /// Whether `flag` was written at all. A command reads what its flags hold this way, never by
    /// asking whether a value is empty.
    pub(crate) fn given(&self, flag: F) -> bool {
        match self {
            Self::Flags(flags) => flags.contains_key(&flag),
            Self::None | Self::RawText(_) => false,
        }
    }

    /// The value `flag` carried: none when it was not written, and none for a flag that takes none.
    pub(crate) fn value(&self, flag: F) -> Option<&str> {
        match self {
            Self::Flags(flags) => flags.get(&flag).and_then(|value| value.as_deref()),
            Self::None | Self::RawText(_) => None,
        }
    }

    /// The payload read verbatim: some only under `Grammar::RawText`, and empty when nothing was
    /// written, which is the command's to interpret.
    pub(crate) fn text(&self) -> Option<&str> {
        match self {
            Self::RawText(text) => Some(text),
            Self::None | Self::Flags(_) => None,
        }
    }
}

/// Reads `payload` the way `grammar` says. The payload is trimmed once here; whitespace inside it is
/// kept as written, and no quoting or escaping is invented.
pub(crate) fn parse<F: FlagName>(payload: &str, grammar: &Grammar) -> Result<Payload<F>, ArgError> {
    let payload = payload.trim();
    match grammar {
        Grammar::None if payload.is_empty() => Ok(Payload::None),
        Grammar::None => Err(ArgError::ExtraPositional(payload.to_string())),
        Grammar::RawText => Ok(Payload::RawText(payload.to_string())),
        Grammar::Flags => read_flags(payload),
    }
}

fn read_flags<F: FlagName>(payload: &str) -> Result<Payload<F>, ArgError> {
    let mut read: HashMap<F, Option<String>> = HashMap::new();
    let mut tokens = payload.split_whitespace();
    while let Some(token) = tokens.next() {
        if token.starts_with('-') {
            let (flag, inline) = read_flag(token)?;
            if read.contains_key(&flag) {
                return Err(ArgError::Duplicate(flag.name()));
            }
            let value = match (flag.takes_value(), inline) {
                (false, Some(_)) => return Err(ArgError::UnexpectedValue(flag.name())),
                (false, None) => None,
                (true, Some(value)) => Some(value.to_string()),
                (true, None) => Some(
                    tokens
                        .next()
                        .ok_or(ArgError::MissingValue(flag.name()))?
                        .to_string(),
                ),
            };
            read.insert(flag, value);
        } else {
            let flag = F::ALL
                .iter()
                .copied()
                .find(|flag| flag.takes_value() && !read.contains_key(flag))
                .ok_or_else(|| ArgError::ExtraPositional(token.to_string()))?;
            read.insert(flag, Some(token.to_string()));
        }
    }
    Ok(Payload::Flags(read))
}

/// Splits one `--name` or `--name=value` token into the flag it names and the value written inline.
fn read_flag<F: FlagName>(token: &str) -> Result<(F, Option<&str>), ArgError> {
    let name = match token.strip_prefix("--") {
        Some(name) if !name.is_empty() => name,
        _ => return Err(ArgError::UnknownFlag(token.to_string())),
    };
    let (name, inline) = match name.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (name, None),
    };
    F::ALL
        .iter()
        .copied()
        .find(|flag| flag.name() == name)
        .map(|flag| (flag, inline))
        .ok_or_else(|| ArgError::UnknownFlag(token.to_string()))
}

/// Why a payload is not a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ArgError {
    /// The token as written, dashes included.
    UnknownFlag(String),
    /// The flag's `name()`, dashes excluded.
    MissingValue(&'static str),
    /// A value written for a flag that takes none.
    UnexpectedValue(&'static str),
    Duplicate(&'static str),
    /// Two flags that cannot both be given.
    Conflict(&'static str, &'static str),
    /// A positional token with no slot left.
    ExtraPositional(String),
    /// A value the flag's own parser refused; `value` is what the person wrote.
    BadValue {
        arg: &'static str,
        value: String,
    },
    /// A value that parsed but sits above the flag's bound.
    OutOfRange {
        arg: &'static str,
        value: String,
        max: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum TestFlag {
        Range,
        Surfaces,
        Reset,
    }

    impl FlagName for TestFlag {
        const ALL: &'static [Self] = &[Self::Range, Self::Surfaces, Self::Reset];

        fn name(self) -> &'static str {
            match self {
                Self::Range => "range",
                Self::Surfaces => "surfaces",
                Self::Reset => "reset",
            }
        }

        fn takes_value(self) -> bool {
            match self {
                Self::Range | Self::Surfaces => true,
                Self::Reset => false,
            }
        }
    }

    const GRAMMAR: Grammar = Grammar::Flags;

    /// A bare token and the flag it fills are one argument, however it was written.
    #[test]
    fn a_flag_and_its_position_are_one_argument() {
        let by_flag =
            parse::<TestFlag>("--range all --surfaces agent,files", &GRAMMAR).expect("readable");
        let by_position = parse::<TestFlag>("all agent,files", &GRAMMAR).expect("readable");
        assert_eq!(by_flag, by_position);
        assert_eq!(by_position.value(TestFlag::Range), Some("all"));
        assert_eq!(by_position.value(TestFlag::Surfaces), Some("agent,files"));
        assert!(!by_position.given(TestFlag::Reset));
    }

    /// The two spellings of one value are one argument too.
    #[test]
    fn an_inline_value_is_the_same_as_a_separate_one() {
        assert_eq!(
            parse::<TestFlag>("--range=7d", &GRAMMAR).expect("readable"),
            parse::<TestFlag>("--range 7d", &GRAMMAR).expect("readable")
        );
    }

    /// A flag that takes no value is read as written, not as an empty string.
    #[test]
    fn a_flag_without_a_value_is_given_and_holds_nothing() {
        let payload = parse::<TestFlag>("--reset", &GRAMMAR).expect("readable");
        assert!(payload.given(TestFlag::Reset));
        assert_eq!(payload.value(TestFlag::Reset), None);
        assert!(!payload.given(TestFlag::Range));
    }

    /// Every refusal names what it refused.
    #[test]
    fn every_refusal_names_its_reason() {
        let cases = [
            ("--bogus 1", ArgError::UnknownFlag("--bogus".into())),
            ("-x", ArgError::UnknownFlag("-x".into())),
            ("--range", ArgError::MissingValue("range")),
            ("--reset=1", ArgError::UnexpectedValue("reset")),
            ("--range a --range b", ArgError::Duplicate("range")),
            ("all x extra", ArgError::ExtraPositional("extra".into())),
        ];
        for (payload, expected) in cases {
            assert_eq!(
                parse::<TestFlag>(payload, &GRAMMAR),
                Err(expected),
                "{payload}"
            );
        }
    }

    /// The two shapes that carry no flags of their own.
    #[test]
    fn the_other_two_grammars_read_their_own_shape() {
        assert_eq!(parse::<NoFlags>("  ", &Grammar::None), Ok(Payload::None));
        assert_eq!(
            parse::<NoFlags>("x", &Grammar::None),
            Err(ArgError::ExtraPositional("x".into()))
        );

        let raw = parse::<NoFlags>("  --verbose is broken  ", &Grammar::RawText).expect("readable");
        assert_eq!(raw.text(), Some("--verbose is broken"));
        let empty = parse::<NoFlags>("", &Grammar::RawText).expect("readable");
        assert_eq!(empty.text(), Some(""));
    }
}
