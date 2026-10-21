//! The command table: one declaration per command.

use crate::{
    agent::command::{
        Invocation,
        args::Grammar,
        menu,
        parse::{self, DIGEST_GRAMMAR, EXPLAIN_GRAMMAR, LANG_GRAMMAR, PURGE_GRAMMAR},
    },
    agentcore::PerCatalog,
};

/// One command the agent offers, as data.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Declaration {
    /// The wire name, without the platform's prefix.
    pub(crate) name: &'static str,
    /// Other spellings the same command answers to.
    pub(crate) aliases: &'static [&'static str],
    /// The menu copy, per client catalog.
    pub(crate) copy: PerCatalog<&'static str>,
    pub(crate) grammar: Grammar,
    /// Complete payloads, written without the command name or the platform prefix: the forms a
    /// person can type as they stand. A page-only form is not an example.
    pub(crate) examples: &'static [&'static str],
    pub(crate) parse: fn(&str) -> Invocation,
}

impl Declaration {
    /// The form a person types, derived from the grammar.
    pub(crate) fn synopsis(&self) -> String {
        self.grammar.synopsis(self.name)
    }

    /// The complete forms as the help prints them, behind the platform's own prefix.
    pub(crate) fn example_lines(&self, prefix: &str) -> Vec<String> {
        self.examples
            .iter()
            .map(|payload| {
                let line = format!("{prefix}{}", self.name);
                if payload.is_empty() {
                    line
                } else {
                    format!("{line} {payload}")
                }
            })
            .collect()
    }
}

/// Every command the agent offers, in the order a menu lists them.
pub(crate) const DECLARATIONS: [Declaration; 6] = [
    Declaration {
        name: "status",
        aliases: &[],
        copy: menu::STATUS_COPY,
        grammar: Grammar::NONE,
        examples: &[],
        parse: parse::status,
    },
    Declaration {
        name: "lang",
        aliases: &[],
        copy: menu::LANG_COPY,
        grammar: LANG_GRAMMAR,
        examples: &["zh", "--reset"],
        parse: parse::lang,
    },
    Declaration {
        name: "purge",
        aliases: &[],
        copy: menu::PURGE_COPY,
        grammar: PURGE_GRAMMAR,
        examples: &["7d --yes", "all --surfaces agent,files --yes"],
        parse: parse::purge,
    },
    Declaration {
        name: "organizememory",
        aliases: &[],
        copy: menu::ORGANIZE_MEMORY_COPY,
        grammar: Grammar::NONE,
        examples: &[],
        parse: parse::organize_memory,
    },
    Declaration {
        name: "explain",
        aliases: &["explan"],
        copy: menu::EXPLAIN_COPY,
        grammar: EXPLAIN_GRAMMAR,
        examples: &["the last plan", "-- --verbose"],
        parse: parse::explain,
    },
    Declaration {
        name: "digest",
        aliases: &[],
        copy: menu::DIGEST_COPY,
        grammar: DIGEST_GRAMMAR,
        examples: &["7d", "7d --to me --lang en"],
        parse: parse::digest,
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

    /// Every command's synopsis is the one a person can type on any platform.
    #[test]
    fn every_command_spells_its_own_form() {
        let synopses: Vec<(&str, String)> = DECLARATIONS
            .iter()
            .map(|declared| (declared.name, declared.synopsis()))
            .collect();
        assert_eq!(
            synopses,
            [
                ("status", "status".to_owned()),
                ("lang", "lang [CODE] [--reset]".to_owned()),
                (
                    "purge",
                    "purge [RANGE] [--surfaces LIST] [--yes]".to_owned()
                ),
                ("organizememory", "organizememory".to_owned()),
                ("explain", "explain [--] [TEXT...]".to_owned()),
                (
                    "digest",
                    "digest [RANGE] [--lang CODE] [--to here|me]".to_owned()
                ),
            ]
        );
    }

    /// Every command has a line that names everything it needs: the form a platform without pages
    /// answers with, and the one a page only shortens.
    #[test]
    fn every_command_has_a_complete_line() {
        let complete = [
            ("status", ""),
            ("lang", "zh"),
            ("purge", "7d --yes"),
            ("organizememory", ""),
            ("explain", "something worth explaining"),
            ("digest", "7d --to here"),
        ];
        for (name, payload) in complete {
            let declared = find(name).expect("a declared command");
            assert!(
                matches!((declared.parse)(payload), Invocation::Ready(_)),
                "/{name} {payload}"
            );
        }
    }

    /// A command that takes a payload shows at least one complete form, and every example reads.
    #[test]
    fn every_argument_command_shows_an_example() {
        for declared in &DECLARATIONS {
            let takes_payload = !declared.grammar.positionals.is_empty()
                || !declared.grammar.options.is_empty()
                || declared.grammar.raw_tail;
            if takes_payload {
                assert!(
                    !declared.examples.is_empty(),
                    "{} takes a payload but shows no example",
                    declared.name
                );
            }
            for example in declared.examples {
                assert!(
                    matches!((declared.parse)(example), Invocation::Ready(_)),
                    "/{} {example}",
                    declared.name
                );
            }
        }
    }

    /// The help prints the examples behind the platform's own prefix, and none for a command that
    /// takes no payload.
    #[test]
    fn examples_carry_the_platform_prefix() {
        let purge = find("purge").expect("a declared command");
        assert_eq!(
            purge.example_lines("/"),
            vec![
                "/purge 7d --yes".to_owned(),
                "/purge all --surfaces agent,files --yes".to_owned(),
            ]
        );
        assert_eq!(
            purge.example_lines("!"),
            vec![
                "!purge 7d --yes".to_owned(),
                "!purge all --surfaces agent,files --yes".to_owned(),
            ]
        );
        assert!(
            find("status")
                .expect("a declared command")
                .example_lines("!")
                .is_empty()
        );
    }
}
