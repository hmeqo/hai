//! The command identity: one declaration per command — its wire names, its menu copy, how a payload
//! is read, and what answering it takes from the platform.

use crate::{
    agent::{
        channel::ChannelCapabilities,
        command::{Invocation, menu, parse},
    },
    agentcore::PerCatalog,
};

/// What answering a command takes from the platform: a reply it can show, or a page of choices it
/// must put out and read a press back from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Interaction {
    Answer,
    Choices,
}

impl Interaction {
    /// Whether a platform with these capabilities can run a command that needs this.
    pub(crate) fn supported_by(self, capabilities: ChannelCapabilities) -> bool {
        match self {
            Self::Answer => true,
            Self::Choices => capabilities.choices,
        }
    }
}

/// One command the agent offers, as data: what a platform advertises, and what the agent reads.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Declaration {
    /// The wire name, without the platform's prefix.
    pub name: &'static str,
    /// Other spellings the same command answers to.
    pub aliases: &'static [&'static str],
    /// The menu copy, per client catalog.
    pub copy: PerCatalog<&'static str>,
    /// How a payload becomes an invocation.
    pub parse: fn(&str) -> Invocation,
    /// What answering it takes from the platform.
    pub interaction: Interaction,
}

/// Every command the agent offers, in the order a menu lists them.
pub(crate) const DECLARATIONS: [Declaration; 6] = [
    Declaration {
        name: "status",
        aliases: &[],
        copy: menu::STATUS_COPY,
        parse: parse::status,
        interaction: Interaction::Answer,
    },
    Declaration {
        name: "lang",
        aliases: &[],
        copy: menu::LANG_COPY,
        parse: parse::lang,
        interaction: Interaction::Answer,
    },
    Declaration {
        name: "purge",
        aliases: &[],
        copy: menu::PURGE_COPY,
        parse: parse::purge,
        interaction: Interaction::Choices,
    },
    Declaration {
        name: "organizememory",
        aliases: &[],
        copy: menu::ORGANIZE_MEMORY_COPY,
        parse: parse::organize_memory,
        interaction: Interaction::Answer,
    },
    Declaration {
        name: "explain",
        aliases: &["explan"],
        copy: menu::EXPLAIN_COPY,
        parse: parse::explain,
        interaction: Interaction::Answer,
    },
    Declaration {
        name: "digest",
        aliases: &[],
        copy: menu::DIGEST_COPY,
        parse: parse::digest,
        interaction: Interaction::Choices,
    },
];

/// Resolves a wire name to its declaration. Names are exact: the command contract does not inherit
/// the platform's spelling habits.
pub(crate) fn find(name: &str) -> Option<&'static Declaration> {
    DECLARATIONS
        .iter()
        .find(|declared| declared.name == name || declared.aliases.contains(&name))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn declarations_are_unique_and_resolve() {
        let mut spellings = HashSet::new();
        for declared in &DECLARATIONS {
            assert!(
                spellings.insert(declared.name),
                "{} is declared twice",
                declared.name
            );
        }
        for declared in &DECLARATIONS {
            assert_eq!(
                find(declared.name).map(|found| found.name),
                Some(declared.name)
            );
            for alias in declared.aliases {
                assert!(spellings.insert(alias), "{alias} collides");
                assert_eq!(
                    find(alias).map(|found| found.name),
                    Some(declared.name),
                    "{alias} resolves away from {}",
                    declared.name
                );
            }
        }

        assert!(find("Lang").is_none());
        assert!(find("").is_none());
        assert!(find("unknown").is_none());
    }

    /// Only a command that draws a page needs a platform that can show one.
    #[test]
    fn only_the_paged_commands_need_choices() {
        let mut paged: Vec<&str> = DECLARATIONS
            .iter()
            .filter(|declared| declared.interaction == Interaction::Choices)
            .map(|declared| declared.name)
            .collect();
        paged.sort_unstable();
        assert_eq!(paged, ["digest", "purge"]);

        let without = ChannelCapabilities {
            forms: &[],
            reference: false,
            quote: false,
            max_media_bytes: 0,
            choices: false,
        };
        let with = ChannelCapabilities {
            choices: true,
            ..without
        };
        for declared in &DECLARATIONS {
            let needs = declared.interaction == Interaction::Choices;
            assert_eq!(
                declared.interaction.supported_by(without),
                !needs,
                "{}",
                declared.name
            );
            assert!(declared.interaction.supported_by(with), "{}", declared.name);
        }
    }
}
