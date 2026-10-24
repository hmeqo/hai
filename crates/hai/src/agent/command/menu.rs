//! The menu copy every command carries.

use crate::agent::prompt::text::PerCatalog;

pub(super) const STATUS_COPY: PerCatalog<&'static str> = PerCatalog {
    zh: "查看运行状态",
    en: "Show the agent status",
};

pub(super) const LANG_COPY: PerCatalog<&'static str> = PerCatalog {
    zh: "设置本会话语言",
    en: "Set the language for this conversation",
};

pub(super) const MODEL_COPY: PerCatalog<&'static str> = PerCatalog {
    zh: "选择本会话的模型",
    en: "Choose this conversation's model",
};

pub(super) const CONTENT_POLICY_COPY: PerCatalog<&'static str> = PerCatalog {
    zh: "设置本会话的内容尺度",
    en: "Set this conversation's content policy",
};

pub(super) const PERSONA_COPY: PerCatalog<&'static str> = PerCatalog {
    zh: "设置本会话的语气与人设",
    en: "Set this conversation's persona",
};

pub(super) const PURGE_COPY: PerCatalog<&'static str> = PerCatalog {
    zh: "清空指定范围的聊天记录、记忆与平台消息",
    en: "Purge this chat's records, memory and platform messages over a range",
};

pub(super) const EXPLAIN_COPY: PerCatalog<&'static str> = PerCatalog {
    zh: "解释指定内容或当前上下文",
    en: "Explain a given target or the current context",
};

pub(super) const DIGEST_COPY: PerCatalog<&'static str> = PerCatalog {
    zh: "写一份近期对话的报告",
    en: "Write a report over the recent conversation",
};
