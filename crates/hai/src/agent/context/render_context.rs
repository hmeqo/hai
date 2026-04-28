//! 上下文数据结构
//!
//! `CommonContext` 是一次 agent 运行的完整上下文快照，由 `ContextFactory` 异步组装。
//! 字段打平（无嵌套 MessageWindow），内置 ID 索引，渲染层直接查询无需额外中间层。

use std::{
    collections::HashMap,
    time::{Duration, SystemTime},
};

use derive_more::Deref;
use jiff::{Timestamp, tz::TimeZone};
use timeago::Formatter;
use uuid::Uuid;

use crate::{
    bot::telegram::BotIdentity,
    domain::{
        entity::{Account, Chat, Message, Perception, Scratchpad, Topic},
        service::memory::RelatedMemory,
        vo::TopicSearchResult,
    },
};

// ─── 主结构 ──────────────────────────────────────────────────────────────────

/// 一次 agent 运行的完整上下文快照（纯数据，不含渲染逻辑）
#[derive(Debug, Deref)]
pub struct RenderContext {
    #[deref]
    pub data: RenderContextData,

    // ── 内置索引（构建时一次性建立，O(1) 查询）──────────────────────────
    accounts_by_id: HashMap<i64, usize>,
    messages_by_id: HashMap<i64, usize>,
    topics_by_id: HashMap<Uuid, usize>,
}

/// 构建 `RenderContext` 所需的输入数据
#[derive(Debug)]
pub struct RenderContextData {
    // ── 身份与环境 ──────────────────────────────────────────────────────
    pub bot: BotIdentity,
    pub chat: Chat,
    pub current_time: String,

    // ── 消息窗口 ────────────────────────────────────────────────────────
    /// 渲染窗口内的消息（含 reply context，按时间排序）
    pub messages: Vec<Message>,
    /// 所有消息 ID（任务完成后标记已读用）
    pub message_ids: Vec<i64>,
    /// 该 chat 未读消息总数（可能大于窗口大小）
    pub total_unread: i64,

    // ── 话题与记忆 ──────────────────────────────────────────────────────
    /// 当前会话的活跃话题
    pub topics: Vec<Topic>,
    /// 向量检索到的相关历史话题
    pub related_topics: Vec<TopicSearchResult>,
    /// 向量检索到的相关记忆
    pub related_memories: Vec<RelatedMemory>,

    // ── 账号 ────────────────────────────────────────────────────────────
    /// 消息中涉及的账号（含同一身份的其他平台账号）
    pub accounts: Vec<Account>,

    // ── 附件理解 ────────────────────────────────────────────────────────
    /// 所有 perception 列表（仅保留 URL 来源的，Resource 来源的直接嵌入 attachment）
    pub perceptions: Vec<Perception>,
    /// attachment_id → Vec<Perception> 映射，首个 occurrence 内嵌全部 <analysis>
    pub perception_by_attachment_id: HashMap<Uuid, Vec<Perception>>,
    /// 重复 resource 的 attachment 指向首个 attachment_id
    pub same_resource_as: HashMap<Uuid, Uuid>,

    // ── 工作记忆 ────────────────────────────────────────────────────────
    pub scratchpad: Option<Scratchpad>,
}

impl RenderContext {
    /// 由 `ContextFactory` 调用，组装好原始数据后一次性建立索引。
    pub(super) fn new(data: RenderContextData) -> Self {
        let accounts_by_id = data
            .accounts
            .iter()
            .enumerate()
            .map(|(i, a)| (a.id, i))
            .collect();
        let messages_by_id = data
            .messages
            .iter()
            .enumerate()
            .map(|(i, m)| (m.id, i))
            .collect();
        let topics_by_id = data
            .topics
            .iter()
            .enumerate()
            .map(|(i, t)| (t.id, i))
            .collect();

        Self {
            data,
            accounts_by_id,
            messages_by_id,
            topics_by_id,
        }
    }

    // ── 索引查询 ─────────────────────────────────────────────────────────

    pub fn get_account(&self, id: i64) -> Option<&Account> {
        self.accounts_by_id.get(&id).map(|&i| &self.accounts[i])
    }

    pub fn get_message(&self, id: i64) -> Option<&Message> {
        self.messages_by_id.get(&id).map(|&i| &self.messages[i])
    }

    pub fn get_topic(&self, id: Uuid) -> Option<&Topic> {
        self.topics_by_id.get(&id).map(|&i| &self.topics[i])
    }

    pub fn perceptions(&self) -> &[Perception] {
        &self.data.perceptions
    }

    /// 获取消息发送者的显示名称
    pub fn sender_name(&self, msg: &Message) -> String {
        let account_id = match msg.account_id {
            Some(id) if id == self.bot.account_id() => return "You".to_string(),
            Some(id) => id,
            None => {
                return if msg.role == "assistant" {
                    "Other Assistant".to_string()
                } else {
                    "User".to_string()
                };
            }
        };

        match self.get_account(account_id) {
            Some(account) => display_name(account, account_id),
            None => format!("User{} [Unknown]", account_id),
        }
    }

    /// 获取消息所属话题的标题（用于渲染 hint）
    pub fn topic_hint(&self, msg: &Message) -> String {
        msg.topic_id
            .and_then(|tid| self.get_topic(tid))
            .and_then(|t| t.title.clone())
            .unwrap_or_default()
    }
}

// ─── 辅助函数（模块私有）─────────────────────────────────────────────────────

pub(super) fn display_name(account: &Account, fallback_id: i64) -> String {
    use crate::domain::vo::PlatformAccountMeta;

    let meta = account
        .meta
        .clone()
        .and_then(|v| serde_json::from_value::<PlatformAccountMeta>(v).ok());

    let Some(meta) = meta else {
        return format!("User{}", fallback_id);
    };

    let full_name = meta.full_name();
    let username = meta.username();

    match username {
        Some(u) => format!("{} (@{})", full_name, u),
        None => full_name,
    }
}

/// 格式化时间戳：今天内显示相对时间，更早显示绝对
pub(crate) fn format_time_dyn(ts: impl Into<Option<Timestamp>>) -> String {
    let Some(ts) = ts.into() else {
        return "None".to_string();
    };
    let now = Timestamp::now();
    if now.duration_since(ts).as_secs() < 86400 {
        format_relative_time(ts)
    } else {
        ts.to_zoned(TimeZone::system()).to_string()
    }
}

/// 格式化时间戳：今天内显示相对时间，更早显示绝对+相对
pub(crate) fn format_time_dyn2(ts: impl Into<Option<Timestamp>>) -> String {
    let Some(ts) = ts.into() else {
        return "None".to_string();
    };
    let now = Timestamp::now();
    if now.duration_since(ts).as_secs() < 86400 {
        format_relative_time(ts)
    } else {
        format!(
            "{} ({})",
            ts.to_zoned(TimeZone::system()),
            format_relative_time(ts)
        )
    }
}

pub(crate) fn format_relative_time(ts: Timestamp) -> String {
    let then = SystemTime::UNIX_EPOCH + Duration::from_secs(ts.as_second() as u64);
    let duration = SystemTime::now().duration_since(then).unwrap_or_default();
    Formatter::new().convert(duration)
}
