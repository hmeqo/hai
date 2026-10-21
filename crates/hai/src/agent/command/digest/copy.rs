//! The words a digest says to a person: what its pages read and what a delivery says.

use super::dialog::{Delivery, DigestDialog};
use crate::{
    agent::host_copy::stretch_text,
    agentcore::{Language, PerCatalog},
    domain::vo::Stretch,
};

/// The labels the pages read, apart from the one the chat's own kind decides.
pub(crate) struct DigestLabels {
    pub custom: &'static str,
    pub start: &'static str,
    pub dm: &'static str,
    pub here: &'static str,
    pub back: &'static str,
    pub close: &'static str,
    pub retry: &'static str,
}

pub(crate) fn labels(language: Language) -> DigestLabels {
    LABELS.get(language.text_catalog())
}

/// The three stretches the page offers, each paired with the label it reads — the same words the
/// page it leads to names the stretch with.
pub(crate) fn presets(language: Language) -> [(Stretch, String); 3] {
    const DAYS: [i64; 3] = [86_400, 7 * 86_400, 30 * 86_400];
    DAYS.map(|secs| {
        let stretch = Stretch::Last { secs };
        (stretch, stretch_text(language, &stretch))
    })
}

/// One page's text: what it asks, and which window it is about.
pub(crate) fn stage_text(language: Language, dialog: &DigestDialog) -> String {
    let catalog = language.text_catalog();
    match dialog {
        DigestDialog::Stretch { .. } => STRETCH_PAGE.get(catalog).into(),
        DigestDialog::Confirm {
            stretch,
            language: report,
            ..
        } => {
            let line = stretch_line(language, *stretch);
            let report = report
                .filter(|report| *report != language)
                .map(|report| report.names().get(catalog).as_str());
            (CONFIRM_PAGE.get(catalog))(line.as_str(), report)
        }
        DigestDialog::Running { stretch, delivery } => (RUNNING_PAGE.get(catalog))(
            stretch_line(language, *stretch).as_str(),
            *delivery == Delivery::Person,
        ),
        DigestDialog::Undelivered {
            target: Delivery::Person,
            ..
        } => UNDELIVERED_DM_PAGE.get(catalog).into(),
        DigestDialog::Undelivered { error, .. } => {
            (UNDELIVERED_CHAT_PAGE.get(catalog))(error.as_ref())
        }
        DigestDialog::Failed { error, .. } => (FAILED_PAGE.get(catalog))(error.as_ref()),
    }
}

/// The advice a person gets when the presets are not the stretch they want.
pub(crate) fn custom_hint(language: Language, min: u32, max: u32) -> String {
    (CUSTOM_HINT.get(language.text_catalog()))(min, max)
}

/// The file a report goes out as, named for the days it covers.
pub(crate) fn filename(stretch: Stretch) -> String {
    let (from, to) = covered_days(stretch);
    format!("digest-{from}_{to}.md")
}

pub(crate) fn closed(language: Language) -> String {
    CLOSED.get(language.text_catalog()).into()
}

/// The document's caption: the window it covers, and the unfinished note when the pass was cut off.
pub(crate) fn caption(language: Language, stretch: Stretch, truncated: bool) -> String {
    let line = stretch_line(language, stretch);
    if truncated {
        format!("{line}\n{}", UNFINISHED.get(language.text_catalog()))
    } else {
        line
    }
}

/// What the private message says when the request is accepted, before the document arrives.
pub(crate) fn dm_queued(language: Language) -> String {
    DM_QUEUED.get(language.text_catalog()).into()
}

/// Why a person-targeted press cannot start: the platform named nobody to deliver to.
pub(crate) fn no_sender(language: Language) -> String {
    NO_SENDER.get(language.text_catalog()).into()
}

/// The pass wrote nothing worth sending.
pub(crate) fn empty(language: Language) -> String {
    EMPTY.get(language.text_catalog()).into()
}

/// The pass spent its step budget before it wrote anything, so the fault is the ceiling and not the
/// material.
pub(crate) fn truncated(language: Language) -> String {
    TRUNCATED.get(language.text_catalog()).into()
}

/// The pass failed, in its own words: the notice a command line gets where a page cannot show it.
pub(crate) fn failed(language: Language, error: &str) -> String {
    (FAILED_PAGE.get(language.text_catalog()))(error)
}

/// The pass wrote its report but the platform refused the delivery: there is no page to keep it on,
/// so the failure is the whole message.
pub(crate) fn undelivered(language: Language, error: &str) -> String {
    (UNDELIVERED_CHAT_PAGE.get(language.text_catalog()))(error)
}

/// The chat agent went away while the pass was running, which a restart explains.
pub(crate) fn interrupted(language: Language) -> String {
    INTERRUPTED.get(language.text_catalog()).into()
}

/// The window as a page reads it: the host's wording for a stretch, plus the two local days it
/// covers.
fn stretch_line(language: Language, stretch: Stretch) -> String {
    let (from, to) = covered_days(stretch);
    let line = RANGE_LINE.get(language.text_catalog());
    format!("{}{}", stretch_text(language, &stretch), line(&from, &to))
}

/// The two local days a stretch covers. A digest is only ever asked for a count of days back from
/// now, so the window always has a start; its end is now, which is what an open upper end means.
pub(super) fn covered_days(stretch: Stretch) -> (String, String) {
    let now = jiff::Timestamp::now();
    let range = stretch.resolve(now);
    let zone = jiff::tz::TimeZone::system();
    let day = |at: jiff::Timestamp| at.to_zoned(zone.clone()).strftime("%Y-%m-%d").to_string();
    let since = range
        .since
        .expect("a digest's stretch is a count of days back from now");
    (day(since), day(range.until.unwrap_or(now)))
}

const LABELS: PerCatalog<DigestLabels> = PerCatalog {
    zh: DigestLabels {
        custom: "自定义…",
        start: "生成报告",
        dm: "私信给我",
        here: "发到本群",
        back: "返回",
        close: "关闭",
        retry: "重试",
    },
    en: DigestLabels {
        custom: "Custom…",
        start: "Write report",
        dm: "DM me",
        here: "Send here",
        back: "Back",
        close: "Close",
        retry: "Retry",
    },
};

const STRETCH_PAGE: PerCatalog<&'static str> = PerCatalog {
    zh: "写一份这段时间的报告。先选时间范围：",
    en: "Write a report over the recent conversation. Which stretch?",
};

const CUSTOM_HINT: PerCatalog<fn(u32, u32) -> String> = PerCatalog {
    zh: custom_hint_zh,
    en: custom_hint_en,
};

fn custom_hint_zh(min: u32, max: u32) -> String {
    format!("自定义范围请发送 `/digest <天数>`（{min}–{max}）。")
}

fn custom_hint_en(min: u32, max: u32) -> String {
    format!("For another stretch send `/digest <days>` ({min}–{max}).")
}

const RANGE_LINE: PerCatalog<fn(&str, &str) -> String> = PerCatalog {
    zh: range_line_zh,
    en: range_line_en,
};

fn range_line_zh(from: &str, to: &str) -> String {
    format!("（{from} – {to}）")
}

fn range_line_en(from: &str, to: &str) -> String {
    format!(" ({from} – {to})")
}

const CONFIRM_PAGE: PerCatalog<fn(&str, Option<&str>) -> String> = PerCatalog {
    zh: |stretch, report| match report {
        Some(name) => format!("将写入报告：{stretch}\n报告语言：{name}"),
        None => format!("将写入报告：{stretch}"),
    },
    en: |stretch, report| match report {
        Some(name) => format!("About to write the report: {stretch}\nReport language: {name}"),
        None => format!("About to write the report: {stretch}"),
    },
};

const RUNNING_PAGE: PerCatalog<fn(&str, bool) -> String> = PerCatalog {
    zh: |stretch, person| match person {
        true => format!("正在写报告：{stretch}\n写好后私信给发起人。"),
        false => format!("正在写报告：{stretch}"),
    },
    en: |stretch, person| match person {
        true => format!(
            "Writing the report: {stretch}\nIt will go to whoever asked, in a private message."
        ),
        false => format!("Writing the report: {stretch}"),
    },
};

const UNDELIVERED_DM_PAGE: PerCatalog<&'static str> = PerCatalog {
    zh: "报告已生成，但私信发不出去：你还没有和 bot 私聊过。先在私聊里发 /start，再点重试。",
    en: "The report is ready, but the private message failed: start a private chat with me first, then press retry.",
};

const UNDELIVERED_CHAT_PAGE: PerCatalog<fn(&str) -> String> = PerCatalog {
    zh: |error| format!("报告已生成，但发不出去：{error}"),
    en: |error| format!("The report is ready, but it could not be delivered: {error}"),
};

const FAILED_PAGE: PerCatalog<fn(&str) -> String> = PerCatalog {
    zh: |error| format!("写报告失败：{error}"),
    en: |error| format!("The report failed: {error}"),
};

const CLOSED: PerCatalog<&'static str> = PerCatalog {
    zh: "已关闭。",
    en: "Closed.",
};

const DM_QUEUED: PerCatalog<&'static str> = PerCatalog {
    zh: "已收到，报告写好后会发到这个私聊。",
    en: "Got it — the report will arrive here when it is ready.",
};

const UNFINISHED: PerCatalog<&'static str> = PerCatalog {
    zh: "未写完：步数用尽。",
    en: "Unfinished: the step budget ran out.",
};

const NO_SENDER: PerCatalog<&'static str> = PerCatalog {
    zh: "没有可以私信的人。",
    en: "there is no one to send a private message to.",
};

const EMPTY: PerCatalog<&'static str> = PerCatalog {
    zh: "这次没有写出任何内容。",
    en: "the digest produced no text.",
};

const TRUNCATED: PerCatalog<&'static str> = PerCatalog {
    zh: "步数用完了，报告还没写完；缩窄时间范围或重试即可。",
    en: "the pass ran out of steps before the report was written; narrow the range or try again.",
};

const INTERRUPTED: PerCatalog<&'static str> = PerCatalog {
    zh: "报告还没写完就中断了，重试即可。",
    en: "the report was cut off; try again.",
};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    /// A digest writes a report: neither catalog calls it a summary on any page or button.
    #[test]
    fn a_digest_writes_a_report() {
        let week = Stretch::Last { secs: 7 * 86_400 };
        let pages = [
            DigestDialog::Stretch {
                language: None,
                delivery: None,
            },
            DigestDialog::Confirm {
                stretch: week,
                language: None,
                delivery: None,
            },
            DigestDialog::Running {
                stretch: week,
                delivery: Delivery::Person,
            },
            DigestDialog::Undelivered {
                stretch: week,
                report: Arc::from("the report"),
                truncated: false,
                target: Delivery::Person,
                error: Arc::from("nowhere to deliver it"),
            },
            DigestDialog::Failed {
                stretch: week,
                error: Arc::from("an error"),
                language: None,
                delivery: Delivery::Chat,
            },
        ];

        for language in [Language::Zh, Language::En] {
            for page in &pages {
                let text = stage_text(language, page);
                assert!(
                    !text.contains("总结") && !text.to_lowercase().contains("summar"),
                    "{page:?} / {language:?} reads as a summary: {text}"
                );
            }
            let labels = labels(language);
            for label in [
                labels.custom,
                labels.start,
                labels.dm,
                labels.here,
                labels.back,
                labels.close,
                labels.retry,
            ] {
                assert!(!label.contains("总结"), "{language:?}: {label}");
            }
        }
    }

    /// A report cut off by the step budget says so, rather than reading as a finished one.
    #[test]
    fn a_truncated_report_says_it_is_unfinished() {
        let week = Stretch::Last { secs: 7 * 86_400 };

        assert!(caption(Language::Zh, week, true).contains("未写完"));
        assert!(caption(Language::En, week, true).contains("Unfinished"));
        assert!(!caption(Language::Zh, week, false).contains("未写完"));
        assert!(!caption(Language::En, week, false).contains("Unfinished"));
    }

    /// An undelivered page names its own reason: the private chat, or the platform's error.
    #[test]
    fn an_undelivered_page_names_why() {
        let page = |target| DigestDialog::Undelivered {
            stretch: Stretch::Last { secs: 7 * 86_400 },
            report: Arc::from("the report"),
            truncated: false,
            target,
            error: Arc::from("the chat is gone"),
        };

        assert!(stage_text(Language::Zh, &page(Delivery::Person)).contains("私聊"));
        assert!(stage_text(Language::En, &page(Delivery::Person)).contains("private chat"));
        assert!(stage_text(Language::Zh, &page(Delivery::Chat)).contains("the chat is gone"));
    }

    /// The confirmation names the report's language only when it differs from the page's own.
    #[test]
    fn the_confirmation_names_a_foreign_report_language() {
        let week = Stretch::Last { secs: 7 * 86_400 };
        let page = |language| DigestDialog::Confirm {
            stretch: week,
            language,
            delivery: None,
        };
        let named = |language: Language, catalog| language.names().get(catalog).as_str().to_owned();

        let foreign = stage_text(Language::En, &page(Some(Language::Ja)));
        let japanese = named(Language::Ja, Language::En.text_catalog());
        assert!(
            foreign.contains(&format!("Report language: {japanese}")),
            "{foreign}"
        );

        let zh = stage_text(Language::Zh, &page(Some(Language::En)));
        let english = named(Language::En, Language::Zh.text_catalog());
        assert!(zh.contains(&format!("报告语言：{english}")), "{zh}");

        for (language, marker) in [
            (Language::En, "Report language"),
            (Language::Zh, "报告语言"),
        ] {
            let same = stage_text(language, &page(Some(language)));
            let none = stage_text(language, &page(None));
            assert!(!same.contains(marker), "{same}");
            assert!(!none.contains(marker), "{none}");
        }
    }

    /// A pass that spent its steps before writing anything says so, rather than reading as empty.
    #[test]
    fn a_truncated_pass_names_the_step_ceiling() {
        let zh = truncated(Language::Zh);
        let en = truncated(Language::En);
        assert!(zh.contains("步数"), "{zh}");
        assert!(en.contains("steps"), "{en}");
        assert_ne!(zh, en);
    }
}
