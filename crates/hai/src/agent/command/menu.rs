//! The menu copy every command carries.

use crate::agentcore::PerCatalog;

pub(super) const STATUS_COPY: PerCatalog<&'static str> = PerCatalog {
    zh: "查看运行状态",
    en: "Show the agent status",
};

pub(super) const LANG_COPY: PerCatalog<&'static str> = PerCatalog {
    zh: "设置聊天语言",
    en: "Set the language for this chat",
};

pub(super) const PURGE_COPY: PerCatalog<&'static str> = PerCatalog {
    zh: "清空指定范围的聊天记录、记忆与 Telegram 消息",
    en: "Purge this chat's records, memory and Telegram messages over a range",
};

pub(super) const ORGANIZE_MEMORY_COPY: PerCatalog<&'static str> = PerCatalog {
    zh: "整理记忆与话题",
    en: "Organize memory and topics",
};

pub(super) const EXPLAIN_COPY: PerCatalog<&'static str> = PerCatalog {
    zh: "解释指定内容或当前上下文",
    en: "Explain a given target or the current context",
};

pub(super) const DIGEST_COPY: PerCatalog<&'static str> = PerCatalog {
    zh: "写一份近期对话的报告",
    en: "Write a report over the recent conversation",
};
