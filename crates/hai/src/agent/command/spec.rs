//! The command table: one declaration per command.

use crate::agent::{
    command::{
        Invocation,
        args::Grammar,
        menu,
        parse::{
            self, CONTENT_POLICY_GRAMMAR, DIGEST_GRAMMAR, EXPLAIN_GRAMMAR, LANG_GRAMMAR,
            MODEL_GRAMMAR, PERSONA_GRAMMAR, PURGE_GRAMMAR,
        },
    },
    prompt::text::PerCatalog,
};

/// The configured switches a declaration can depend on. A command whose gate is off is absent from
/// the menu, help and parser rather than present-and-refusing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CommandGates {
    pub allow_persona_change: bool,
    pub allow_content_policy_change: bool,
}

impl CommandGates {
    #[cfg(test)]
    pub const ALL: Self = Self {
        allow_persona_change: true,
        allow_content_policy_change: true,
    };

    fn allows(self, gate: Gate) -> bool {
        match gate {
            Gate::Always => true,
            Gate::PersonaChange => self.allow_persona_change,
            Gate::ContentPolicyChange => self.allow_content_policy_change,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gate {
    Always,
    PersonaChange,
    ContentPolicyChange,
}

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
    gate: Gate,
}

impl Declaration {
    fn available(&self, gates: CommandGates) -> bool {
        gates.allows(self.gate)
    }

    /// The same form under the spelling the person actually used: a platform's alias must not
    /// turn an answer about `/contentpolicy` into a usage line naming an unusable spelling.
    pub(crate) fn synopsis_as(&self, name: &str) -> String {
        self.grammar.synopsis(name)
    }

    /// The complete forms as the help prints them, behind the platform's own prefix.
    pub(crate) fn example_lines_as(&self, prefix: &str, name: &str) -> Vec<String> {
        self.examples
            .iter()
            .map(|payload| {
                let line = format!("{prefix}{name}");
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
pub(crate) const DECLARATIONS: [Declaration; 8] = [
    Declaration {
        name: "status",
        aliases: &[],
        copy: menu::STATUS_COPY,
        grammar: Grammar::NONE,
        examples: &[],
        parse: parse::status,
        gate: Gate::Always,
    },
    Declaration {
        name: "lang",
        aliases: &[],
        copy: menu::LANG_COPY,
        grammar: LANG_GRAMMAR,
        examples: &["zh", "--reset"],
        parse: parse::lang,
        gate: Gate::Always,
    },
    Declaration {
        name: "model",
        aliases: &[],
        copy: menu::MODEL_COPY,
        grammar: MODEL_GRAMMAR,
        examples: &["fast", "--reset"],
        parse: parse::model,
        gate: Gate::Always,
    },
    Declaration {
        name: "content-policy",
        // Telegram's menu vocabulary allows lowercase letters, digits and underscores only.
        aliases: &["contentpolicy"],
        copy: menu::CONTENT_POLICY_COPY,
        grammar: CONTENT_POLICY_GRAMMAR,
        examples: &["mature", "--reset"],
        parse: parse::content_policy,
        gate: Gate::ContentPolicyChange,
    },
    Declaration {
        name: "persona",
        aliases: &[],
        copy: menu::PERSONA_COPY,
        grammar: PERSONA_GRAMMAR,
        examples: &["speak plainly", "--reset"],
        parse: parse::persona,
        gate: Gate::PersonaChange,
    },
    Declaration {
        name: "purge",
        aliases: &[],
        copy: menu::PURGE_COPY,
        grammar: PURGE_GRAMMAR,
        examples: &["7d --yes", "all --surfaces agent,files --yes"],
        parse: parse::purge,
        gate: Gate::Always,
    },
    Declaration {
        name: "explain",
        aliases: &["explan"],
        copy: menu::EXPLAIN_COPY,
        grammar: EXPLAIN_GRAMMAR,
        examples: &["the last plan", "-- --verbose"],
        parse: parse::explain,
        gate: Gate::Always,
    },
    Declaration {
        name: "digest",
        aliases: &[],
        copy: menu::DIGEST_COPY,
        grammar: DIGEST_GRAMMAR,
        examples: &["7d", "7d --to me --lang en"],
        parse: parse::digest,
        gate: Gate::Always,
    },
];

/// Resolves an available wire name to its declaration. Names are exact: the command contract does not
/// inherit the platform's spelling habits.
pub(crate) fn find(name: &str, gates: CommandGates) -> Option<&'static Declaration> {
    DECLARATIONS.iter().find(|declared| {
        (declared.name == name || declared.aliases.contains(&name)) && declared.available(gates)
    })
}

/// The declarations a menu and help may show, in table order.
pub(crate) fn declarations(gates: CommandGates) -> impl Iterator<Item = &'static Declaration> {
    DECLARATIONS
        .iter()
        .filter(move |declared| declared.available(gates))
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
                find(declared.name, CommandGates::ALL).map(|found| found.name),
                Some(declared.name)
            );
            for alias in declared.aliases {
                assert!(spellings.insert(alias), "{alias} collides");
                assert_eq!(
                    find(alias, CommandGates::ALL).map(|found| found.name),
                    Some(declared.name),
                    "{alias} resolves away from {}",
                    declared.name
                );
            }
        }

        assert!(find("Lang", CommandGates::ALL).is_none());
        assert!(find("", CommandGates::ALL).is_none());
        assert!(find("unknown", CommandGates::ALL).is_none());
    }

    /// Every command's synopsis is the one a person can type on any platform.
    #[test]
    fn every_command_spells_its_own_form() {
        let synopses: Vec<(&str, String)> = DECLARATIONS
            .iter()
            .map(|declared| (declared.name, declared.synopsis_as(declared.name)))
            .collect();
        assert_eq!(
            synopses,
            [
                ("status", "status".to_owned()),
                ("lang", "lang [CODE] [--reset]".to_owned()),
                ("model", "model [ALIAS] [--reset]".to_owned()),
                (
                    "content-policy",
                    "content-policy [PRESET] [--reset]".to_owned()
                ),
                ("persona", "persona [--] [TEXT...]".to_owned()),
                (
                    "purge",
                    "purge [RANGE] [--surfaces LIST] [--yes]".to_owned()
                ),
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
            ("model", "fast"),
            ("content-policy", "mature"),
            ("persona", "speak plainly"),
            ("purge", "7d --yes"),
            ("explain", "something worth explaining"),
            ("digest", "7d --to here"),
        ];
        for (name, payload) in complete {
            let declared = find(name, CommandGates::ALL).expect("a declared command");
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
        let purge = find("purge", CommandGates::ALL).expect("a declared command");
        assert_eq!(
            purge.example_lines_as("/", "purge"),
            vec![
                "/purge 7d --yes".to_owned(),
                "/purge all --surfaces agent,files --yes".to_owned(),
            ]
        );
        assert_eq!(
            purge.example_lines_as("!", "purge"),
            vec![
                "!purge 7d --yes".to_owned(),
                "!purge all --surfaces agent,files --yes".to_owned(),
            ]
        );
        assert!(
            find("status", CommandGates::ALL)
                .expect("a declared command")
                .example_lines_as("!", "status")
                .is_empty()
        );
    }

    /// A disabled change command is absent, not present-and-refusing.
    #[test]
    fn gates_hide_commands_they_forbid() {
        let gates = CommandGates {
            allow_persona_change: false,
            allow_content_policy_change: false,
        };
        assert!(find("content-policy", gates).is_none());
        assert!(find("contentpolicy", gates).is_none());
        assert!(find("persona", gates).is_none());
        assert!(find("status", gates).is_some());
    }

    /// A help answer echoes the spelling that reached it, so Telegram's `/contentpolicy` never
    /// answers with a usage line the client cannot run.
    #[test]
    fn help_keeps_the_alias_that_was_used() {
        let content_policy =
            find("contentpolicy", CommandGates::ALL).expect("the Telegram alias resolves");
        assert_eq!(
            content_policy.synopsis_as("contentpolicy"),
            "contentpolicy [PRESET] [--reset]"
        );
        assert_eq!(
            content_policy.example_lines_as("/", "contentpolicy"),
            vec![
                "/contentpolicy mature".to_owned(),
                "/contentpolicy --reset".to_owned(),
            ]
        );
        assert_eq!(
            content_policy.example_lines_as("/", "content-policy"),
            vec![
                "/content-policy mature".to_owned(),
                "/content-policy --reset".to_owned(),
            ]
        );
    }
}
