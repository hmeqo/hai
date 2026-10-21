use crate::{
    agent::command::ArgError,
    agentcore::{Language, LanguageName, PerCatalog},
    domain::vo::{Stretch, TimeUnit, whole_units},
};

/// How a stretch reads in a chat's language: everything, a relative span in the largest whole unit
/// that divides it, or an absolute one as the two local times it covers. Every command that names a
/// stretch reads it here — a range button and the page it leads to are the same words.
pub(crate) fn stretch_text(language: Language, stretch: &Stretch) -> String {
    match *stretch {
        Stretch::All => STRETCH_ALL.get(language.text_catalog()).into(),
        Stretch::Last { secs } => (STRETCH_LAST.get(language.text_catalog()))(secs),
        Stretch::Between { from, to } => {
            let zone = jiff::tz::TimeZone::system();
            format!(
                "{} – {}",
                from.to_zoned(zone.clone()).strftime("%Y-%m-%d %H:%M"),
                to.to_zoned(zone).strftime("%Y-%m-%d %H:%M"),
            )
        }
    }
}

const STRETCH_ALL: PerCatalog<&'static str> = PerCatalog {
    zh: "全部时间",
    en: "all time",
};

const STRETCH_LAST: PerCatalog<fn(i64) -> String> = PerCatalog {
    zh: stretch_last_zh,
    en: stretch_last_en,
};

fn stretch_last_zh(secs: i64) -> String {
    let (count, unit) = whole_units(secs);
    match unit {
        TimeUnit::Days => format!("近 {count} 天"),
        TimeUnit::Hours => format!("近 {count} 小时"),
        TimeUnit::Minutes => format!("近 {count} 分钟"),
        TimeUnit::Seconds => format!("近 {count} 秒"),
    }
}

fn stretch_last_en(secs: i64) -> String {
    let (count, unit) = whole_units(secs);
    match unit {
        TimeUnit::Days => last_in(count, "day"),
        TimeUnit::Hours => last_in(count, "hour"),
        TimeUnit::Minutes => last_in(count, "minute"),
        TimeUnit::Seconds => last_in(count, "second"),
    }
}

/// "the last hour" reads as a phrase; more than one of a unit is a count.
fn last_in(count: i64, unit: &str) -> String {
    if count == 1 {
        format!("the last {unit}")
    } else {
        format!("the last {count} {unit}s")
    }
}

/// Every reply the host composes itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HostCopy {
    Unauthorized,
    Greeting,
    /// What `/lang` did. None of them carries a language: the reply names the effective one.
    LanguageCurrent,
    LanguageSet,
    LanguageReset,
    LanguageUnsupported,
    /// The agent's own status block, which is absent when the agent is not running.
    Status(Option<String>),
    BadArgs {
        error: ArgError,
        usage: String,
    },
    Usage {
        about: &'static PerCatalog<&'static str>,
        usage: String,
        examples: Vec<String>,
    },
    /// A button whose page is gone, and one whose dialog belongs to somebody else.
    DialogStale,
    DialogForeign,
}

impl HostCopy {
    /// `language` is both the language the reply is written in and, for the language replies, the
    /// language they are about.
    pub(crate) fn render(self, language: Language) -> String {
        let catalog = language.text_catalog();
        let name = language.names().get(catalog);
        match self {
            Self::Unauthorized => UNAUTHORIZED.get(catalog).into(),
            Self::Greeting => GREETING.get(catalog).into(),
            Self::LanguageCurrent => (LANGUAGE_CURRENT.get(catalog))(name),
            Self::LanguageSet => (LANGUAGE_SET.get(catalog))(name),
            Self::LanguageReset => (LANGUAGE_RESET.get(catalog))(name),
            Self::LanguageUnsupported => LANGUAGE_UNSUPPORTED.get(catalog).into(),
            Self::Status(None) => STATUS_EMPTY.get(catalog).into(),
            Self::Status(Some(body)) => body,
            Self::BadArgs { error, usage } => {
                format!("{}\n{usage}", (BAD_ARGS.get(catalog))(&error))
            }
            Self::Usage {
                about,
                usage,
                examples,
            } => {
                let mut text = format!("{}\n{usage}", about.get(catalog));
                if !examples.is_empty() {
                    text.push_str("\n\n");
                    text.push_str(EXAMPLES.get(catalog));
                    for example in examples {
                        text.push('\n');
                        text.push_str(&example);
                    }
                }
                text
            }
            Self::DialogStale => DIALOG_STALE.get(catalog).into(),
            Self::DialogForeign => DIALOG_FOREIGN.get(catalog).into(),
        }
    }
}

const UNAUTHORIZED: PerCatalog<&'static str> = PerCatalog {
    zh: "你没有使用这个机器人的权限。如有疑问，请联系机器人所有者。",
    en: concat!(
        "You are not authorized to use this bot. ",
        "If you think this is a mistake, please contact the bot owner."
    ),
};

const GREETING: PerCatalog<&'static str> = PerCatalog {
    zh: "你好！在的，有事就说。",
    en: "Hello! I am here — talk to me whenever.",
};

const STATUS_EMPTY: PerCatalog<&'static str> = PerCatalog {
    zh: "状态  未运行",
    en: "status  not running",
};

const BAD_ARGS: PerCatalog<fn(&ArgError) -> String> = PerCatalog {
    zh: bad_args_zh,
    en: bad_args_en,
};

/// A press the engine cannot place: the page is gone, or it never belonged to the person pressing.
/// Neither sentence names a command — a token does not say which one drew the button.
const DIALOG_STALE: PerCatalog<&'static str> = PerCatalog {
    zh: "这个按钮已经失效，重新发送命令即可。",
    en: "that button expired; send the command again.",
};

const DIALOG_FOREIGN: PerCatalog<&'static str> = PerCatalog {
    zh: "这个对话框不是你的。",
    en: "that dialog belongs to somebody else.",
};

const EXAMPLES: PerCatalog<&'static str> = PerCatalog {
    zh: "示例：",
    en: "Examples:",
};

fn bad_args_zh(err: &ArgError) -> String {
    match err {
        ArgError::UnknownOption(token) => format!("未知参数 `{token}`。"),
        ArgError::MissingValue(name) => format!("`{}` 需要一个值。", name.render()),
        ArgError::UnexpectedValue(name) => format!("`{}` 不接受值。", name.render()),
        ArgError::Duplicate(name) => format!("`{}` 给了两次。", name.render()),
        ArgError::Conflict(first, second) => {
            format!("`{}` 和 `{}` 不能同时给。", first.render(), second.render())
        }
        ArgError::ExtraPositional(token) => format!("多出来的参数：`{token}`。"),
        ArgError::MissingArgument(name) => format!("缺少 `{}`。", name.render()),
        ArgError::BadValue { arg, value } => {
            format!("`{}` 读不出值：`{value}`。", arg.render())
        }
        ArgError::OutOfRange {
            arg,
            value,
            min,
            max,
        } => format!("`{}` 应在 {min}–{max} 之间，收到 `{value}`。", arg.render()),
    }
}

fn bad_args_en(err: &ArgError) -> String {
    match err {
        ArgError::UnknownOption(token) => format!("unknown argument `{token}`."),
        ArgError::MissingValue(name) => format!("`{}` needs a value.", name.render()),
        ArgError::UnexpectedValue(name) => format!("`{}` takes no value.", name.render()),
        ArgError::Duplicate(name) => format!("`{}` was given twice.", name.render()),
        ArgError::Conflict(first, second) => format!(
            "`{}` and `{}` cannot both be given.",
            first.render(),
            second.render()
        ),
        ArgError::ExtraPositional(token) => format!("unexpected argument `{token}`."),
        ArgError::MissingArgument(name) => format!("`{}` is missing.", name.render()),
        ArgError::BadValue { arg, value } => {
            format!("`{}` cannot read `{value}`.", arg.render())
        }
        ArgError::OutOfRange {
            arg,
            value,
            min,
            max,
        } => format!("`{}` takes {min}–{max}; got `{value}`.", arg.render()),
    }
}

const LANGUAGE_UNSUPPORTED: PerCatalog<&'static str> = PerCatalog {
    zh: "无法识别的语言代码。",
    en: "Unknown language code.",
};

const LANGUAGE_CURRENT: PerCatalog<fn(LanguageName) -> String> = PerCatalog {
    zh: language_current_zh,
    en: language_current_en,
};

const LANGUAGE_SET: PerCatalog<fn(LanguageName) -> String> = PerCatalog {
    zh: language_set_zh,
    en: language_set_en,
};

const LANGUAGE_RESET: PerCatalog<fn(LanguageName) -> String> = PerCatalog {
    zh: language_reset_zh,
    en: language_reset_en,
};

fn language_current_zh(name: LanguageName) -> String {
    format!("当前语言：{}", name.as_str())
}

fn language_current_en(name: LanguageName) -> String {
    format!("Current language: {}", name.as_str())
}

fn language_set_zh(name: LanguageName) -> String {
    format!("语言已设为{}。", name.as_str())
}

fn language_set_en(name: LanguageName) -> String {
    format!("Language set to {}.", name.as_str())
}

fn language_reset_zh(name: LanguageName) -> String {
    format!("已恢复默认：{}。", name.as_str())
}

fn language_reset_en(name: LanguageName) -> String {
    format!("Back to the default: {}.", name.as_str())
}

#[cfg(test)]
mod tests {
    use strum::IntoEnumIterator;

    use super::*;

    /// A reply naming a language names it the way the catalog it is written in spells it.
    #[test]
    fn language_replies_name_the_language_in_their_own_set() {
        let naming = [
            HostCopy::LanguageCurrent,
            HostCopy::LanguageSet,
            HostCopy::LanguageReset,
        ];
        for language in Language::iter() {
            let name = language.names().get(language.text_catalog()).as_str();
            for copy in naming.clone() {
                assert!(
                    copy.clone().render(language).contains(name),
                    "{copy:?} / {language:?} does not name {name}"
                );
            }
        }
    }

    /// An unreadable argument is answered with the argument and the reason, and the form follows.
    #[test]
    fn bad_args_name_the_argument_and_the_reason() {
        let err = ArgError::MissingValue(crate::agent::command::ArgName::Option("surfaces"));
        let usage = "purge [RANGE] [--surfaces LIST] [--yes]";
        let zh = HostCopy::BadArgs {
            error: err.clone(),
            usage: usage.to_owned(),
        }
        .render(Language::Zh);
        let en = HostCopy::BadArgs {
            error: err,
            usage: usage.to_owned(),
        }
        .render(Language::En);
        assert!(zh.contains("`--surfaces`"), "{zh}");
        assert!(en.contains("`--surfaces`"), "{en}");
        assert!(zh.ends_with(usage), "{zh}");
        assert!(en.ends_with(usage), "{en}");
        assert_ne!(zh, en);
    }

    /// A command line that stopped before a required argument is answered as such, with the form it
    /// should have had; the host says nothing about the platform's UI.
    #[test]
    fn a_missing_argument_is_named_with_the_form() {
        let usage = "!purge 7d --yes";
        let error = ArgError::MissingArgument(crate::agent::command::ArgName::Option("yes"));
        let zh = HostCopy::BadArgs {
            error: error.clone(),
            usage: usage.to_owned(),
        }
        .render(Language::Zh);
        let en = HostCopy::BadArgs {
            error,
            usage: usage.to_owned(),
        }
        .render(Language::En);
        assert!(zh.contains("--yes"), "{zh}");
        assert!(en.contains("--yes"), "{en}");
        assert!(zh.ends_with(usage), "{zh}");
        assert!(en.ends_with(usage), "{en}");
    }

    /// A command's help carries its complete forms under the synopsis, in the chat's language.
    #[test]
    fn usage_lists_examples() {
        const ABOUT: PerCatalog<&'static str> = PerCatalog {
            zh: "清理指定范围",
            en: "Purge a range",
        };
        let copy = HostCopy::Usage {
            about: &ABOUT,
            usage: "/purge [RANGE] [--surfaces LIST] [--yes]".to_owned(),
            examples: vec!["/purge 7d --yes".to_owned()],
        };
        let zh = copy.clone().render(Language::Zh);
        let en = copy.render(Language::En);
        assert!(zh.contains("示例：\n/purge 7d --yes"), "{zh}");
        assert!(en.contains("Examples:\n/purge 7d --yes"), "{en}");
        assert!(zh.starts_with("清理指定范围\n/purge [RANGE]"), "{zh}");
        assert!(en.starts_with("Purge a range\n/purge [RANGE]"), "{en}");
    }

    /// A span is named in the unit that divides it exactly, so a five-minute window never reads as
    /// a rounded-down zero and the ladder never skips a unit.
    #[test]
    fn a_stretch_reads_in_whole_units() {
        let named = [
            (300, "近 5 分钟", "the last 5 minutes"),
            (1_800, "近 30 分钟", "the last 30 minutes"),
            (3_600, "近 1 小时", "the last hour"),
            (86_400, "近 1 天", "the last day"),
            (7 * 86_400, "近 7 天", "the last 7 days"),
            (30 * 86_400, "近 30 天", "the last 30 days"),
        ];
        for (secs, zh, en) in named {
            let stretch = Stretch::Last { secs };
            assert_eq!(stretch_text(Language::Zh, &stretch), zh, "{secs} seconds");
            assert_eq!(stretch_text(Language::En, &stretch), en, "{secs} seconds");
        }
        assert_eq!(stretch_text(Language::Zh, &Stretch::All), "全部时间");
        assert_eq!(stretch_text(Language::En, &Stretch::All), "all time");
    }
}
