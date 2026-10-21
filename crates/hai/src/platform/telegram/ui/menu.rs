//! The menu scopes to push: the catalog a client with no scope of its own reads, then one per client
//! language code reading another, because Telegram hands a client the most specific scope it matches.

use strum::IntoEnumIterator;
use teloxide::{Bot, payloads::SetMyCommandsSetters, prelude::Requester, types::BotCommand};

use super::super::{inbound::command::OWN_COMMANDS, wire::adapter::TELEGRAM_CAPABILITIES};
use crate::{
    agent::{channel::ChannelCapabilities, command::DECLARATIONS},
    agentcore::{Language, TextCatalog},
};

/// The catalog a client whose language has no scope of its own reads.
const FALLBACK: TextCatalog = TextCatalog::En;

/// Each scope as the client language code Telegram matches by and the catalog it reads; `None` is the
/// default scope, which every language without a scope of its own reads. The bot composes two
/// catalogs, so only Chinese needs a code.
pub(in crate::platform::telegram) fn scopes() -> Vec<(Option<&'static str>, TextCatalog)> {
    let mut scopes = vec![(None, FALLBACK)];
    for language in Language::iter() {
        let (code, catalog) = (language.client_code(), language.text_catalog());
        if catalog != FALLBACK && !scopes.iter().any(|(pushed, _)| *pushed == Some(code)) {
            scopes.push((Some(code), catalog));
        }
    }
    scopes
}

/// The menu Telegram shows a client: the commands this platform owns outright, then the agent's
/// declarations the channel can actually run, in declaration order. The name goes without the slash —
/// Telegram's `BotCommand.command` allows only lowercase letters, digits and underscores — and the
/// client renders the slash itself.
pub(in crate::platform::telegram) fn bot_commands(
    catalog: TextCatalog,
    capabilities: ChannelCapabilities,
) -> Vec<BotCommand> {
    let mut commands: Vec<BotCommand> = OWN_COMMANDS
        .iter()
        .map(|(name, copy)| BotCommand::new(*name, copy.get(catalog)))
        .collect();
    commands.extend(
        DECLARATIONS
            .iter()
            .filter(|declared| declared.interaction.supported_by(capabilities))
            .map(|declared| BotCommand::new(declared.name, declared.copy.get(catalog))),
    );
    commands
}

/// Pushes every menu scope. A failure only warns: the bot still answers commands, it just shows an
/// older menu.
pub(in crate::platform::telegram) async fn install(bot: &Bot) {
    for (language_code, catalog) in scopes() {
        let request = bot.set_my_commands(bot_commands(catalog, TELEGRAM_CAPABILITIES));
        let result = match language_code {
            Some(code) => request.language_code(code).await,
            None => request.await,
        };
        if let Err(e) = result {
            tracing::warn!(error = %e, language_code = ?language_code, "Failed to set bot commands");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_and_chinese_are_the_only_scopes() {
        assert_eq!(
            scopes(),
            vec![(None, TextCatalog::En), (Some("zh"), TextCatalog::Zh)]
        );
    }

    #[test]
    fn menu_lists_every_command_by_its_bare_name() {
        for catalog in TextCatalog::iter() {
            let listed: Vec<String> = bot_commands(catalog, TELEGRAM_CAPABILITIES)
                .into_iter()
                .map(|command| command.command)
                .collect();

            assert_eq!(
                listed.first().map(String::as_str),
                Some("start"),
                "the platform's own command comes first"
            );
            for declared in &DECLARATIONS {
                assert!(
                    listed.contains(&declared.name.to_owned()),
                    "{} is missing from the menu",
                    declared.name
                );
            }
            assert_eq!(
                listed.len(),
                DECLARATIONS.len() + OWN_COMMANDS.len(),
                "the menu lists the two tables and nothing else"
            );
        }
    }

    /// A channel that cannot put out a page of buttons does not advertise the commands that need one.
    #[test]
    fn a_platform_without_choices_hides_the_paged_commands() {
        let listed: Vec<String> = bot_commands(
            TextCatalog::En,
            ChannelCapabilities {
                choices: false,
                ..TELEGRAM_CAPABILITIES
            },
        )
        .into_iter()
        .map(|command| command.command)
        .collect();

        assert_eq!(
            listed,
            ["start", "status", "lang", "organizememory", "explain"]
        );
    }
}
