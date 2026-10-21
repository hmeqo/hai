//! The flags each command's payload declares, and the grammar that reads them.

use strum::{EnumString, IntoStaticStr};

use crate::agent::command::args::FlagName;

/// Every flag `/lang` declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub(super) enum LangFlag {
    Code,
    Reset,
}

impl FlagName for LangFlag {
    const ALL: &'static [Self] = &[Self::Code, Self::Reset];

    fn name(self) -> &'static str {
        self.into()
    }

    fn takes_value(self) -> bool {
        match self {
            Self::Code => true,
            Self::Reset => false,
        }
    }
}

/// Every flag `/purge` declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub(super) enum PurgeFlag {
    Range,
    Surfaces,
}

impl FlagName for PurgeFlag {
    const ALL: &'static [Self] = &[Self::Range, Self::Surfaces];

    fn name(self) -> &'static str {
        self.into()
    }

    fn takes_value(self) -> bool {
        match self {
            Self::Range | Self::Surfaces => true,
        }
    }
}

/// Every flag `/digest` declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub(super) enum DigestFlag {
    Days,
    Lang,
}

impl FlagName for DigestFlag {
    const ALL: &'static [Self] = &[Self::Days, Self::Lang];

    fn name(self) -> &'static str {
        self.into()
    }

    fn takes_value(self) -> bool {
        match self {
            Self::Days | Self::Lang => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, str::FromStr};

    use super::*;

    /// A flag is spelled once: the name it answers to and the name it is written as agree, and no
    /// two flags of one command share a spelling.
    #[test]
    fn flag_names_match_their_spelling() {
        fn one_apiece<F: FlagName + FromStr + std::fmt::Debug>()
        where
            F::Err: std::fmt::Debug,
        {
            let mut names = HashSet::new();
            for flag in F::ALL {
                let name = flag.name();
                assert!(names.insert(name), "{name} is spelled twice");
                assert_eq!(&F::from_str(name).expect("its own name"), flag);
            }
        }

        one_apiece::<LangFlag>();
        one_apiece::<PurgeFlag>();
        one_apiece::<DigestFlag>();
    }
}
