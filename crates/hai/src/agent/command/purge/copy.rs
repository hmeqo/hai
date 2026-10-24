//! Every word the purge speaks: the confirmation pages, the receipts, and the toast for a button
//! whose page is gone.
use super::{PurgeReport, dialog::PurgeDialog};
use crate::{
    agent::{
        host_copy::stretch_text,
        prompt::text::{LanguageText, PerCatalog},
    },
    config::conversation::Language,
    domain::vo::{PurgeSpec, PurgeSurface, PurgeSurfaces, Stretch},
};

/// The labels the pages share between kinds.
pub(crate) struct PurgeLabels {
    pub range: &'static str,
    pub content: &'static str,
    pub back: &'static str,
    pub confirm: &'static str,
    pub cancel: &'static str,
}

pub(crate) fn labels(language: Language) -> PurgeLabels {
    LABELS.get(language.text_catalog())
}

const STRETCHES: [Stretch; 7] = [
    Stretch::Last { secs: 300 },
    Stretch::Last { secs: 1_800 },
    Stretch::Last { secs: 3_600 },
    Stretch::Last { secs: 86_400 },
    Stretch::Last { secs: 7 * 86_400 },
    Stretch::Last { secs: 30 * 86_400 },
    Stretch::All,
];

/// The stretches the range page offers, in the order they are shown, each paired with the label it
/// reads. A label is the same text the page it leads to names the stretch with, so the button and
/// the confirmation cannot drift apart.
pub(crate) fn stretches(language: Language) -> [(Stretch, String); 7] {
    STRETCHES.map(|stretch| (stretch, stretch_text(language, &stretch)))
}

/// The four surfaces, each paired with the label it reads, in the order they are walked.
pub(crate) fn surfaces(language: Language) -> [(PurgeSurface, &'static str); 4] {
    let labels = SURFACE_LABELS.get(language.text_catalog());
    std::array::from_fn(|i| (PurgeSurface::ALL[i], labels[i]))
}

/// The four surfaces as a check list, one per line.
fn surface_list(language: Language, selected: &PurgeSurfaces) -> String {
    surfaces(language)
        .into_iter()
        .map(|(surface, label)| {
            format!(
                "{} {label}",
                if selected.get(surface) { "✅" } else { "⬜" }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// One page's text: what it is asking and what it would purge as things stand.
pub(crate) fn stage_text(language: Language, dialog: &PurgeDialog) -> String {
    let catalog = language.text_catalog();
    let listed = surface_list(language, &dialog.surfaces());
    match *dialog {
        PurgeDialog::Menu { .. } => format!(
            "{}\n{listed}\n{}",
            CONTENT_HEADING.get(catalog),
            MENU_ASK.get(catalog)
        ),
        PurgeDialog::Stretch { .. } => format!(
            "{}\n{listed}\n{}",
            CONTENT_HEADING.get(catalog),
            STRETCH_ASK.get(catalog)
        ),
        PurgeDialog::Surfaces { .. } => format!("{}\n{listed}", SURFACES_PAGE.get(catalog)),
        PurgeDialog::Confirm { range, .. } => format!(
            "{}\n{listed}\n{}",
            (CONFIRM_PAGE.get(catalog))(stretch_text(language, &range).as_str()),
            CONFIRM_ASK.get(catalog),
        ),
    }
}

pub(crate) fn cancelled(language: Language) -> String {
    CANCELLED.get(language.text_catalog()).into()
}

/// The page a running purge leaves behind.
pub(crate) fn started(language: Language, spec: &PurgeSpec) -> String {
    (STARTED.get(language.text_catalog()))(stretch_text(language, &spec.range).as_str())
}

/// What one purge removed. Only the non-empty items are listed; whether the platform's own messages
/// were touched is named only when they were.
pub(crate) fn report(language: Language, report: &PurgeReport) -> String {
    let catalog = language.text_catalog();
    let counts = report.counts;
    let listed: Vec<String> = [
        (counts.messages, ITEM_MESSAGES),
        (counts.memories, ITEM_MEMORIES),
        (counts.topics, ITEM_TOPICS),
        (counts.scheduled, ITEM_SCHEDULED),
        (counts.events, ITEM_EVENTS),
        (counts.perceptions, ITEM_PERCEPTIONS),
        (report.files_removed, ITEM_FILES),
        (report.cached_removed, ITEM_CACHED),
    ]
    .into_iter()
    .filter(|(removed, _)| *removed > 0)
    .map(|(removed, label)| format!("{} {removed}", label.get(catalog)))
    .collect();

    let mut text = if listed.is_empty() {
        NOTHING.get(catalog).into()
    } else {
        format!("{}{}", PURGED.get(catalog), listed.join(" · "))
    };

    if report.spec.surfaces.telegram {
        let line =
            (PLATFORM.get(catalog))(report.telegram.deleted, report.telegram.failed.len() as u64);
        text.push('\n');
        text.push_str(&line);
    }
    text
}

/// A purge that did not run: the failure's own words, never a rewording.
pub(crate) fn failed(language: Language, error: &str) -> String {
    (FAILED.get(language.text_catalog()))(error)
}

/// The confirmed purge could not be stored, so the Conversation's agent was never rung.
pub(crate) fn not_queued(language: Language) -> String {
    NOT_QUEUED.get(language.text_catalog()).into()
}

const LABELS: PerCatalog<PurgeLabels> = PerCatalog {
    zh: PurgeLabels {
        range: "时间范围…",
        content: "清理内容…",
        back: "返回",
        confirm: "确认执行",
        cancel: "取消",
    },
    en: PurgeLabels {
        range: "Time range…",
        content: "What to purge…",
        back: "Back",
        confirm: "Confirm",
        cancel: "Cancel",
    },
};

const SURFACE_LABELS: PerCatalog<[&'static str; 4]> = PerCatalog {
    zh: ["agent 痕迹", "工作区文件", "平台消息", "审计与转录"],
    en: [
        "agent records",
        "workspace files",
        "platform messages",
        "audit & transcripts",
    ],
};

/// The heading every page above the confirmation carries: what this purge would hold.
const CONTENT_HEADING: PerCatalog<&'static str> = PerCatalog {
    zh: "清理内容：",
    en: "Content:",
};

/// What the first page asks: which of the two questions to answer.
const MENU_ASK: PerCatalog<&'static str> = PerCatalog {
    zh: "先选时间范围，或先调整清理内容。",
    en: "Pick a time range first, or adjust what it purges.",
};

const STRETCH_ASK: PerCatalog<&'static str> = PerCatalog {
    zh: "要清理哪一段？清理后不可恢复。",
    en: "Which stretch? This cannot be undone.",
};

const SURFACES_PAGE: PerCatalog<&'static str> = PerCatalog {
    zh: "清理内容（点一下切换）：",
    en: "What to purge (tap to toggle):",
};

const CONFIRM_PAGE: PerCatalog<fn(&str) -> String> = PerCatalog {
    zh: confirm_page_zh,
    en: confirm_page_en,
};

fn confirm_page_zh(range: &str) -> String {
    format!("将清理：{range}\n内容：")
}

fn confirm_page_en(range: &str) -> String {
    format!("About to purge: {range}\nContent:")
}

const CONFIRM_ASK: PerCatalog<&'static str> = PerCatalog {
    zh: "不可恢复，确认执行？",
    en: "This cannot be undone — confirm?",
};

const CANCELLED: PerCatalog<&'static str> = PerCatalog {
    zh: "已取消。",
    en: "Cancelled.",
};

const STARTED: PerCatalog<fn(&str) -> String> = PerCatalog {
    zh: started_zh,
    en: started_en,
};

fn started_zh(range: &str) -> String {
    format!("开始清理：{range}")
}

fn started_en(range: &str) -> String {
    format!("Purging: {range}")
}

const NOTHING: PerCatalog<&'static str> = PerCatalog {
    zh: "没有可清理的内容。",
    en: "Nothing to purge.",
};

const PURGED: PerCatalog<&'static str> = PerCatalog {
    zh: "已清理：",
    en: "Purged: ",
};

const ITEM_MESSAGES: PerCatalog<&'static str> = PerCatalog {
    zh: "消息",
    en: "messages",
};

const ITEM_MEMORIES: PerCatalog<&'static str> = PerCatalog {
    zh: "记忆",
    en: "memories",
};

const ITEM_TOPICS: PerCatalog<&'static str> = PerCatalog {
    zh: "话题",
    en: "topics",
};

const ITEM_SCHEDULED: PerCatalog<&'static str> = PerCatalog {
    zh: "计划任务",
    en: "scheduled",
};

const ITEM_EVENTS: PerCatalog<&'static str> = PerCatalog {
    zh: "事件",
    en: "events",
};

const ITEM_PERCEPTIONS: PerCatalog<&'static str> = PerCatalog {
    zh: "转录",
    en: "transcripts",
};

const ITEM_FILES: PerCatalog<&'static str> = PerCatalog {
    zh: "工作区文件",
    en: "workspace files",
};

const ITEM_CACHED: PerCatalog<&'static str> = PerCatalog {
    zh: "缓存",
    en: "cached",
};

const PLATFORM: PerCatalog<fn(u64, u64) -> String> = PerCatalog {
    zh: platform_zh,
    en: platform_en,
};

fn platform_zh(deleted: u64, failed: u64) -> String {
    if failed == 0 {
        format!("平台：删除 {deleted} 条")
    } else {
        format!("平台：删除 {deleted} 条，{failed} 条未能删除")
    }
}

fn platform_en(deleted: u64, failed: u64) -> String {
    if failed == 0 {
        format!("Platform: {deleted} deleted")
    } else {
        format!("Platform: {deleted} deleted, {failed} could not be deleted")
    }
}

const NOT_QUEUED: PerCatalog<&'static str> = PerCatalog {
    zh: "没能提交这次清理，请重试。",
    en: "The purge could not be queued; try again.",
};

const FAILED: PerCatalog<fn(&str) -> String> = PerCatalog {
    zh: failed_zh,
    en: failed_en,
};

fn failed_zh(error: &str) -> String {
    format!("清理失败：{error}")
}

fn failed_en(error: &str) -> String {
    format!("Purge failed: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        agent::channel::DeleteReport,
        domain::vo::{PurgeCounts, PurgeSurfaces},
    };

    fn wiped(surfaces: PurgeSurfaces, counts: PurgeCounts, telegram: DeleteReport) -> PurgeReport {
        PurgeReport {
            spec: PurgeSpec {
                range: Stretch::All,
                surfaces,
            },
            counts,
            telegram,
            files_removed: 0,
            cached_removed: 0,
            consumed: Vec::new(),
        }
    }

    /// A receipt names what went and stays quiet about what did not: a zero is not information.
    #[test]
    fn a_receipt_lists_only_what_was_removed() {
        let counts = PurgeCounts {
            messages: 3,
            memories: 1,
            ..PurgeCounts::default()
        };
        let text = report(
            Language::En,
            &wiped(
                PurgeSurfaces {
                    telegram: false,
                    ..PurgeSurfaces::all()
                },
                counts,
                DeleteReport::default(),
            ),
        );

        assert!(text.contains("messages 3"), "{text}");
        assert!(text.contains("memories 1"), "{text}");
        assert!(!text.contains("topics"), "{text}");
        assert!(
            !text.contains("Platform"),
            "a surface that was not purged is not named: {text}"
        );
    }

    /// Nothing found is said plainly rather than as an empty list.
    #[test]
    fn an_empty_receipt_says_there_was_nothing() {
        let text = report(
            Language::Zh,
            &wiped(
                PurgeSurfaces {
                    telegram: false,
                    ..PurgeSurfaces::all()
                },
                PurgeCounts::default(),
                DeleteReport::default(),
            ),
        );

        assert_eq!(text, NOTHING.zh);
    }

    /// What the platform refused is reported as a count; the reasons go to the log, one line each.
    #[test]
    fn a_platform_line_names_what_could_not_be_deleted() {
        let failed = DeleteReport {
            deleted: 4,
            failed: Vec::new(),
        };
        let clean = report(
            Language::En,
            &wiped(PurgeSurfaces::all(), PurgeCounts::default(), failed),
        );
        assert!(clean.contains("Platform: 4 deleted"), "{clean}");
        assert!(!clean.contains("could not"), "{clean}");

        let partial = DeleteReport {
            deleted: 4,
            failed: vec![crate::agent::channel::FailedDeletion {
                external_id: "7".to_owned(),
                detail: "message to delete not found".to_owned(),
            }],
        };
        let text = report(
            Language::En,
            &wiped(PurgeSurfaces::all(), PurgeCounts::default(), partial),
        );
        assert!(text.contains("4 deleted, 1 could not be deleted"), "{text}");
        assert!(
            !text.contains("message to delete not found"),
            "the platform's reason belongs in the log: {text}"
        );
    }
}
