//! Operator view and out-of-band writes for a Conversation: `conversation 2` shows the member
//! table and settings, while `conversation 2 lang en` reads or writes one setting.

use strum::IntoEnumIterator;

use crate::{
    agent::settings::{ResolvedConversationSettings, resolve},
    cli::{ConversationAction, ConversationArgs},
    config::{
        AppConfig,
        conversation::{ContentPolicy, Language},
    },
    domain::{
        repo::Repos,
        service::{
            ConversationService, ConversationSummary, PreferenceService, SettingChange,
            SettingsChange, SettingsService,
        },
        vo::{ChatId, ConversationId, ConversationPreferences, ModelAlias},
    },
    error::{AppError, Result},
};

#[derive(Debug, PartialEq, Eq)]
enum Command {
    List,
    Show(ConversationId),
    Read {
        conversation: ConversationId,
        setting: SettingView,
    },
    Write {
        conversation: ConversationId,
        setting: SettingView,
        change: SettingChange,
    },
    Attach {
        chat: ChatId,
        conversation: ConversationId,
    },
    Detach {
        chat: ChatId,
    },
}

pub(crate) async fn execute(args: ConversationArgs, config: &AppConfig) -> Result<()> {
    let command = plan(args)?;
    let pool = sqlx::PgPool::connect(&config.database.url).await?;
    let repos = Repos::new(pool);
    let service = ConversationService::new(repos.clone());
    match command {
        Command::List => {
            let conversations = service.list().await?;
            if conversations.is_empty() {
                println!("no conversations");
                return Ok(());
            }
            for line in list_lines(&conversations) {
                println!("{line}");
            }
        }
        Command::Show(conversation) => {
            let summary = find(&service, conversation).await?;
            let prefs = read_prefs(&repos, conversation).await?;
            print_show(&summary, &prefs, config);
        }
        Command::Read {
            conversation,
            setting,
        } => {
            find(&service, conversation).await?;
            let prefs = read_prefs(&repos, conversation).await?;
            println!("{}", setting.line(&prefs, &resolve(config, &prefs), config));
        }
        Command::Write {
            conversation,
            setting,
            change,
        } => {
            let summary = find(&service, conversation).await?;
            let origin = summary
                .chats
                .iter()
                .map(|chat| chat.id)
                .min()
                .ok_or_else(|| {
                    AppError::message(format!(
                        "conversation {conversation} has no Chat; its settings cannot be announced"
                    ))
                })?;
            let prefs = SettingsService::new(repos)
                .apply(config, conversation, origin, change)
                .await?;
            println!("{}", setting.line(&prefs, &resolve(config, &prefs), config));
        }
        Command::Attach { chat, conversation } => {
            service.attach(chat, conversation).await?;
            println!("Chat {chat} moved to Conversation {conversation}");
        }
        Command::Detach { chat } => {
            let conversation = service.detach(chat).await?;
            println!("Chat {chat} moved to Conversation {conversation}");
        }
    }
    Ok(())
}

/// Maps the clap grammar onto the one action it means; every invalid combination fails here, before
/// a database connection exists.
fn plan(args: ConversationArgs) -> Result<Command> {
    let ConversationArgs {
        conversation,
        action,
    } = args;
    match (conversation, action) {
        (None, None) => Err(AppError::message(
            "conversation needs a Conversation id or a command",
        )),
        (None, Some(ConversationAction::List)) => Ok(Command::List),
        (None, Some(ConversationAction::Attach { chat, conversation })) => Ok(Command::Attach {
            chat: ChatId(chat),
            conversation: ConversationId(conversation),
        }),
        (None, Some(ConversationAction::Detach { chat })) => {
            Ok(Command::Detach { chat: ChatId(chat) })
        }
        (None, Some(_)) => Err(AppError::message(
            "a setting command needs a Conversation id: hai conversation <CONVERSATION> <setting>",
        )),
        (Some(conversation), None) => Ok(Command::Show(ConversationId(conversation))),
        (Some(_), Some(ConversationAction::List)) => {
            Err(AppError::message("list does not take a Conversation id"))
        }
        (Some(_), Some(ConversationAction::Attach { .. })) => Err(AppError::message(
            "attach does not take a leading Conversation id; use --conversation",
        )),
        (Some(_), Some(ConversationAction::Detach { .. })) => {
            Err(AppError::message("detach does not take a Conversation id"))
        }
        (Some(conversation), Some(ConversationAction::Lang { code, reset })) => setting_command(
            conversation,
            SettingView::Language,
            code,
            reset.reset,
            parse_language,
            SettingChange::Language,
        ),
        (Some(conversation), Some(ConversationAction::Model { alias, reset })) => setting_command(
            conversation,
            SettingView::Model,
            alias,
            reset.reset,
            parse_model_alias,
            SettingChange::Model,
        ),
        (Some(conversation), Some(ConversationAction::ContentPolicy { level, reset })) => {
            setting_command(
                conversation,
                SettingView::ContentPolicy,
                level,
                reset.reset,
                parse_content_policy,
                SettingChange::ContentPolicy,
            )
        }
        (Some(conversation), Some(ConversationAction::Persona { text, reset })) => setting_command(
            conversation,
            SettingView::Persona,
            text,
            reset.reset,
            parse_persona,
            SettingChange::Persona,
        ),
    }
}

fn setting_command<T>(
    conversation: i64,
    setting: SettingView,
    value: Option<String>,
    reset: bool,
    parse: fn(&str) -> Result<T>,
    change: fn(SettingsChange<T>) -> SettingChange,
) -> Result<Command> {
    if reset {
        if value.is_some() {
            return Err(AppError::message(format!(
                "--reset cannot be combined with a value for {}",
                setting.label()
            )));
        }
        return Ok(Command::Write {
            conversation: ConversationId(conversation),
            setting,
            change: change(SettingsChange::Reset),
        });
    }
    match value {
        Some(value) => Ok(Command::Write {
            conversation: ConversationId(conversation),
            setting,
            change: change(SettingsChange::Set(parse(&value)?)),
        }),
        None => Ok(Command::Read {
            conversation: ConversationId(conversation),
            setting,
        }),
    }
}

async fn find(service: &ConversationService, id: ConversationId) -> Result<ConversationSummary> {
    service
        .list()
        .await?
        .into_iter()
        .find(|conversation| conversation.id == id)
        .ok_or_else(|| AppError::message(format!("conversation {id} not found")))
}

async fn read_prefs(
    repos: &Repos,
    conversation: ConversationId,
) -> Result<ConversationPreferences> {
    Ok(PreferenceService::new(repos.clone())
        .get(conversation)
        .await?
        .unwrap_or_default())
}

/// One header line and one row per member Chat; a Conversation without a Chat keeps its row so it
/// stays visible. No cell is truncated.
fn list_lines(conversations: &[ConversationSummary]) -> Vec<String> {
    let header = [
        "CONVERSATION",
        "CHAT",
        "CHANNEL",
        "TYPE",
        "NAME",
        "EXTERNAL",
        "ALIAS",
    ];
    let mut rows: Vec<[String; 7]> = Vec::new();
    for conversation in conversations {
        if conversation.chats.is_empty() {
            rows.push([
                conversation.id.to_string(),
                "-".into(),
                "-".into(),
                "-".into(),
                "(no chat)".into(),
                "-".into(),
                "-".into(),
            ]);
        }
        for chat in &conversation.chats {
            rows.push([
                conversation.id.to_string(),
                chat.id.to_string(),
                chat.channel_key.to_string(),
                chat.chat_type.as_str().into(),
                chat.name.clone().unwrap_or_else(|| "-".into()),
                chat.external_id.clone(),
                chat.alias.clone(),
            ]);
        }
    }
    let widths = header
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            rows.iter()
                .map(|row| row[index].chars().count())
                .chain(std::iter::once(cell.chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    let mut lines = vec![join_row(header.map(str::to_owned), &widths)];
    lines.extend(rows.into_iter().map(|row| join_row(row, &widths)));
    lines
}

fn join_row(cells: [String; 7], widths: &[usize]) -> String {
    cells
        .iter()
        .enumerate()
        .map(|(index, cell)| format!("{cell:<width$}", width = widths[index]))
        .collect::<Vec<_>>()
        .join("  ")
        .trim_end()
        .to_owned()
}

fn print_show(summary: &ConversationSummary, prefs: &ConversationPreferences, config: &AppConfig) {
    for line in list_lines(std::slice::from_ref(summary)) {
        println!("{line}");
    }
    for line in settings_lines(prefs, &resolve(config, prefs), config) {
        println!("{line}");
    }
}

fn settings_lines(
    prefs: &ConversationPreferences,
    resolved: &ResolvedConversationSettings,
    config: &AppConfig,
) -> Vec<String> {
    let mut lines = vec!["settings".to_owned()];
    lines.extend(
        SettingView::ALL.map(|setting| format!("  {}", setting.line(prefs, resolved, config))),
    );
    lines
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettingView {
    Language,
    Model,
    ContentPolicy,
    Persona,
}

impl SettingView {
    const ALL: [Self; 4] = [
        Self::Language,
        Self::Model,
        Self::ContentPolicy,
        Self::Persona,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Language => "language",
            Self::Model => "model",
            Self::ContentPolicy => "content-policy",
            Self::Persona => "persona",
        }
    }

    fn line(
        self,
        prefs: &ConversationPreferences,
        resolved: &ResolvedConversationSettings,
        config: &AppConfig,
    ) -> String {
        let (value, stored) = match self {
            Self::Language => (
                resolved.language.as_str().to_owned(),
                prefs.language.is_some(),
            ),
            Self::Model => {
                let value = resolved.model.as_ref().map_or_else(
                    || config.model_roles.default.model.clone(),
                    |model| model.to_string(),
                );
                (value, prefs.model.is_some())
            }
            Self::ContentPolicy => (
                resolved.content_policy.as_str().to_owned(),
                prefs.content_policy.is_some(),
            ),
            Self::Persona => (
                resolved.persona.as_deref().unwrap_or("none").to_owned(),
                prefs.persona.is_some(),
            ),
        };
        format!("{:<15} = {} ({})", self.label(), value, source(stored))
    }
}

fn source(stored: bool) -> &'static str {
    if stored { "conversation" } else { "default" }
}

fn parse_language(code: &str) -> Result<Language> {
    code.parse::<Language>().map_err(|_| {
        AppError::message(format!(
            "unknown language code {code:?}; use one of {}",
            Language::iter()
                .map(Language::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })
}

fn parse_model_alias(alias: &str) -> Result<ModelAlias> {
    Ok(ModelAlias::new(alias))
}

fn parse_content_policy(value: &str) -> Result<ContentPolicy> {
    value.parse::<ContentPolicy>().map_err(|_| {
        AppError::message(format!(
            "unknown content level {value:?}; use one of {}",
            ContentPolicy::ALL
                .iter()
                .map(|policy| policy.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })
}

fn parse_persona(text: &str) -> Result<String> {
    Ok(text.to_owned())
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use strum::IntoEnumIterator;

    use super::{
        Command, SettingView, list_lines, parse_content_policy, parse_language, plan,
        settings_lines,
    };
    use crate::{
        cli::{Cli, Commands, ConversationAction, ConversationArgs, ResetArg},
        config::{
            AppConfig,
            conversation::{ContentPolicy, Language},
        },
        domain::{
            model::{ChatType, Platform},
            service::{ChatSummary, ConversationSummary, SettingChange, SettingsChange},
            vo::{ChannelKey, ChatId, ConversationId, ConversationPreferences, ModelAlias},
        },
    };

    fn chat(
        id: i64,
        platform: Platform,
        chat_type: ChatType,
        name: Option<&str>,
        channel: &str,
        external: &str,
        alias: &str,
    ) -> ChatSummary {
        ChatSummary {
            id: ChatId(id),
            alias: alias.into(),
            external_id: external.into(),
            channel_key: ChannelKey::new(platform.as_str(), channel),
            platform,
            chat_type,
            name: name.map(str::to_owned),
        }
    }

    fn args(conversation: Option<i64>, action: Option<ConversationAction>) -> ConversationArgs {
        ConversationArgs {
            conversation,
            action,
        }
    }

    fn reset() -> ResetArg {
        ResetArg { reset: true }
    }

    fn value() -> ResetArg {
        ResetArg { reset: false }
    }

    #[test]
    fn membership_is_one_aligned_row_per_chat() {
        let conversations = vec![
            ConversationSummary {
                id: ConversationId(1),
                chats: vec![chat(
                    8,
                    Platform::Teamspeak,
                    ChatType::Channel,
                    Some("General"),
                    "teamspeak",
                    "channel:1",
                    "chat-8",
                )],
            },
            ConversationSummary {
                id: ConversationId(2),
                chats: vec![chat(
                    9,
                    Platform::Telegram,
                    ChatType::Group,
                    Some("Team"),
                    "42",
                    "42",
                    "chat-9",
                )],
            },
            ConversationSummary {
                id: ConversationId(3),
                chats: Vec::new(),
            },
        ];
        assert_eq!(
            list_lines(&conversations),
            vec![
                "CONVERSATION  CHAT  CHANNEL              TYPE     NAME       EXTERNAL   ALIAS",
                "1             8     teamspeak:teamspeak  channel  General    channel:1  chat-8",
                "2             9     telegram:42          group    Team       42         chat-9",
                "3             -     -                    -        (no chat)  -          -",
            ]
        );
    }

    #[test]
    fn settings_name_their_source() {
        let mut config = AppConfig::default();
        config.model_roles.default.model = "cfg/main".into();
        config.conversation.language = Language::Zh;
        config.conversation.content_policy = ContentPolicy::Safe;
        config.conversation.persona = Some("configured".into());

        let stored = ConversationPreferences {
            language: Some(Language::En),
            model: Some(ModelAlias::new("main")),
            content_policy: Some(ContentPolicy::Standard),
            persona: Some("stored".into()),
        };
        assert_eq!(
            settings_lines(
                &stored,
                &crate::agent::settings::resolve(&config, &stored),
                &config
            ),
            vec![
                "settings",
                "  language        = en (conversation)",
                "  model           = main (conversation)",
                "  content-policy  = standard (conversation)",
                "  persona         = stored (conversation)",
            ]
        );

        let absent = ConversationPreferences::default();
        assert_eq!(
            settings_lines(
                &absent,
                &crate::agent::settings::resolve(&config, &absent),
                &config
            ),
            vec![
                "settings",
                "  language        = zh (default)",
                "  model           = cfg/main (default)",
                "  content-policy  = safe (default)",
                "  persona         = configured (default)",
            ]
        );
    }

    #[test]
    fn a_single_setting_line_has_no_indent() {
        let config = AppConfig::default();
        let prefs = ConversationPreferences::default();
        let resolved = crate::agent::settings::resolve(&config, &prefs);
        assert_eq!(
            SettingView::Language.line(&prefs, &resolved, &config),
            format!("{:<15} = {} ({})", "language", "zh", "default")
        );
    }

    fn planned(conversation: i64, action: ConversationAction) -> Command {
        plan(args(Some(conversation), Some(action))).expect("planned")
    }

    #[test]
    fn plan_maps_each_setting_read_write_and_reset() {
        assert_eq!(
            planned(
                2,
                ConversationAction::Lang {
                    code: Some("en".into()),
                    reset: value(),
                }
            ),
            Command::Write {
                conversation: ConversationId(2),
                setting: SettingView::Language,
                change: SettingChange::Language(SettingsChange::Set(Language::En)),
            }
        );
        assert_eq!(
            planned(
                2,
                ConversationAction::Lang {
                    code: None,
                    reset: reset(),
                }
            ),
            Command::Write {
                conversation: ConversationId(2),
                setting: SettingView::Language,
                change: SettingChange::Language(SettingsChange::Reset),
            }
        );
        assert_eq!(
            planned(
                2,
                ConversationAction::Lang {
                    code: None,
                    reset: value(),
                }
            ),
            Command::Read {
                conversation: ConversationId(2),
                setting: SettingView::Language,
            }
        );
        assert_eq!(
            planned(
                2,
                ConversationAction::Model {
                    alias: Some("main".into()),
                    reset: value(),
                }
            ),
            Command::Write {
                conversation: ConversationId(2),
                setting: SettingView::Model,
                change: SettingChange::Model(SettingsChange::Set(ModelAlias::new("main"))),
            }
        );
        assert_eq!(
            planned(
                2,
                ConversationAction::ContentPolicy {
                    level: Some("standard".into()),
                    reset: value(),
                }
            ),
            Command::Write {
                conversation: ConversationId(2),
                setting: SettingView::ContentPolicy,
                change: SettingChange::ContentPolicy(SettingsChange::Set(ContentPolicy::Standard)),
            }
        );
        assert_eq!(
            planned(
                2,
                ConversationAction::Persona {
                    text: Some("hello".into()),
                    reset: value(),
                }
            ),
            Command::Write {
                conversation: ConversationId(2),
                setting: SettingView::Persona,
                change: SettingChange::Persona(SettingsChange::Set("hello".into())),
            }
        );
    }

    #[test]
    fn plan_rejects_mixed_object_and_membership_commands() {
        assert_eq!(
            plan(args(Some(2), Some(ConversationAction::List)))
                .expect_err("list takes no id")
                .to_string(),
            "list does not take a Conversation id"
        );
        assert_eq!(
            plan(args(
                Some(2),
                Some(ConversationAction::Attach {
                    chat: 1,
                    conversation: 3,
                })
            ))
            .expect_err("attach takes no leading id")
            .to_string(),
            "attach does not take a leading Conversation id; use --conversation"
        );
        assert_eq!(
            plan(args(
                None,
                Some(ConversationAction::Lang {
                    code: None,
                    reset: value(),
                })
            ))
            .expect_err("setting needs an id")
            .to_string(),
            "a setting command needs a Conversation id: hai conversation <CONVERSATION> <setting>"
        );
    }

    #[test]
    fn plan_rejects_a_value_with_reset() {
        assert_eq!(
            plan(args(
                Some(2),
                Some(ConversationAction::Lang {
                    code: Some("en".into()),
                    reset: reset(),
                })
            ))
            .expect_err("value and reset")
            .to_string(),
            "--reset cannot be combined with a value for language"
        );
    }

    #[test]
    fn reset_is_not_a_value_in_the_cli() {
        let error = plan(args(
            Some(2),
            Some(ConversationAction::Lang {
                code: Some("reset".into()),
                reset: value(),
            }),
        ))
        .expect_err("reset is a value, not a reset word");
        assert!(
            error
                .to_string()
                .contains("unknown language code \"reset\""),
            "{error}"
        );
    }

    #[test]
    fn clap_parses_object_first_and_the_alias() {
        let cli = Cli::try_parse_from(["hai", "conversation", "2", "lang", "en"])
            .expect("object-first grammar");
        let Some(Commands::Conversation { args }) = cli.command else {
            panic!("conversation command");
        };
        assert_eq!(args.conversation, Some(2));
        assert!(matches!(
            &args.action,
            Some(ConversationAction::Lang {
                code: Some(code),
                reset: ResetArg { reset: false },
            }) if code == "en"
        ));

        let cli = Cli::try_parse_from(["hai", "conv", "2", "model"]).expect("alias");
        let Some(Commands::Conversation { args }) = cli.command else {
            panic!("conversation command");
        };
        assert_eq!(args.conversation, Some(2));
        assert!(matches!(
            &args.action,
            Some(ConversationAction::Model {
                alias: None,
                reset: ResetArg { reset: false },
            })
        ));
    }

    #[test]
    fn clap_rejects_the_action_first_spelling() {
        assert!(
            Cli::try_parse_from(["hai", "conversation", "lang", "2", "en"]).is_err(),
            "only the object-first spelling is accepted"
        );
    }

    #[test]
    fn clap_rejects_a_value_with_reset() {
        assert!(
            Cli::try_parse_from(["hai", "conversation", "2", "lang", "en", "--reset"]).is_err()
        );
    }

    #[test]
    fn clap_escapes_persona_text_after_a_double_dash() {
        let cli = Cli::try_parse_from(["hai", "conversation", "2", "persona", "--", "--reset"])
            .expect("escaped persona");
        let Some(Commands::Conversation { args }) = cli.command else {
            panic!("conversation command");
        };
        assert!(matches!(
            &args.action,
            Some(ConversationAction::Persona {
                text: Some(text),
                reset: ResetArg { reset: false },
            }) if text == "--reset"
        ));
    }

    #[test]
    fn an_unknown_language_lists_the_codes() {
        let error = parse_language("xx").expect_err("unknown language");
        assert_eq!(
            error.to_string(),
            format!(
                "unknown language code \"xx\"; use one of {}",
                Language::iter()
                    .map(Language::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        );
    }

    #[test]
    fn an_unknown_content_level_lists_the_levels() {
        let error = parse_content_policy("mature").expect_err("removed level");
        assert_eq!(
            error.to_string(),
            "unknown content level \"mature\"; use one of safe, standard, unrestricted"
        );
    }

    #[test]
    fn language_parsing_is_case_insensitive() {
        assert_eq!(parse_language("EN").expect("language"), Language::En);
        assert_eq!(parse_language("en-gb").expect("language"), Language::EnGb);
    }

    #[test]
    fn every_content_level_parses() {
        for level in ContentPolicy::ALL {
            assert_eq!(parse_content_policy(level.as_str()).expect("level"), level);
        }
    }
}
