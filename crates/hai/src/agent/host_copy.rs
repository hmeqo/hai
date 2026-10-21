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
    /// Why a command's payload could not be read.
    BadArgs(ArgError),
    /// A button whose page is gone, and one whose dialog belongs to somebody else.
    DialogStale,
    DialogForeign,
    /// A command this channel cannot run: the platform cannot put out the page it needs.
    Unsupported(&'static str),
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
            Self::BadArgs(err) => (BAD_ARGS.get(catalog))(&err),
            Self::DialogStale => DIALOG_STALE.get(catalog).into(),
            Self::DialogForeign => DIALOG_FOREIGN.get(catalog).into(),
            Self::Unsupported(name) => (UNSUPPORTED.get(catalog))(name),
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

/// A command whose page this channel cannot put out: the name it was typed under, in the chat's own
/// language.
const UNSUPPORTED: PerCatalog<fn(&str) -> String> = PerCatalog {
    zh: |name| format!("当前平台不支持 /{name}。"),
    en: |name| format!("this platform does not support /{name}."),
};

fn bad_args_zh(err: &ArgError) -> String {
    match err {
        ArgError::UnknownFlag(token) => format!("未知参数 `{token}`。"),
        ArgError::MissingValue(name) => format!("`--{name}` 需要一个值。"),
        ArgError::UnexpectedValue(name) => format!("`--{name}` 不接受值。"),
        ArgError::Duplicate(name) => format!("`--{name}` 给了两次。"),
        ArgError::Conflict(first, second) => format!("`--{first}` 和 `--{second}` 不能同时给。"),
        ArgError::ExtraPositional(token) => format!("多出来的参数：`{token}`。"),
        ArgError::BadValue { arg, value } => format!("`--{arg}` 读不出值：`{value}`。"),
        ArgError::OutOfRange { arg, value, max } => {
            format!("`--{arg}` 最多 {max}，收到 `{value}`。")
        }
    }
}

fn bad_args_en(err: &ArgError) -> String {
    match err {
        ArgError::UnknownFlag(token) => format!("unknown argument `{token}`."),
        ArgError::MissingValue(name) => format!("`--{name}` needs a value."),
        ArgError::UnexpectedValue(name) => format!("`--{name}` takes no value."),
        ArgError::Duplicate(name) => format!("`--{name}` was given twice."),
        ArgError::Conflict(first, second) => {
            format!("`--{first}` and `--{second}` cannot both be given.")
        }
        ArgError::ExtraPositional(token) => format!("unexpected argument `{token}`."),
        ArgError::BadValue { arg, value } => format!("`--{arg}` cannot read `{value}`."),
        ArgError::OutOfRange { arg, value, max } => {
            format!("`--{arg}` takes at most {max}; got `{value}`.")
        }
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

    /// An unreadable argument is answered with the argument and the reason, in the language the
    /// reply is spoken in.
    #[test]
    fn bad_args_name_the_argument_and_the_reason() {
        let err = ArgError::MissingValue("days");
        let zh = HostCopy::BadArgs(err.clone()).render(Language::Zh);
        let en = HostCopy::BadArgs(err).render(Language::En);
        assert!(zh.contains("`--days`"), "{zh}");
        assert!(en.contains("`--days`"), "{en}");
        assert_ne!(zh, en);
    }

    /// A command this channel cannot run is named as it was typed, in the chat's own words.
    #[test]
    fn an_unsupported_command_is_named() {
        let zh = HostCopy::Unsupported("purge").render(Language::Zh);
        let en = HostCopy::Unsupported("purge").render(Language::En);
        assert!(zh.contains("/purge"), "{zh}");
        assert!(en.contains("/purge"), "{en}");
        assert_ne!(zh, en);
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
