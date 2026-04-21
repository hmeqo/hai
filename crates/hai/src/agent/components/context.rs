//! 渲染上下文
//!
//! 将分散的数据打包，并提供简单的查询逻辑

use std::borrow::Cow;
use std::collections::HashMap;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use uuid::Uuid;

use crate::domain::entity::{Account, Message, Topic};
use crate::domain::vo::PlatformAccountMeta;

/// 账户信息（从 Account 提取的可复用结构）
#[derive(Debug, Clone)]
pub struct AccountInfo {
    pub id: i64,
    pub platform: String,
    pub username: Option<String>,
    pub full_name: Option<String>,
    pub identity_id: Option<Uuid>,
}

impl AccountInfo {
    pub fn from_account(account: &Account) -> Self {
        let (username, full_name) = extract_account_meta(account);
        Self {
            id: account.id,
            platform: account.platform.to_string(),
            username,
            full_name,
            identity_id: account.identity_id,
        }
    }

    pub fn display_name(&self, fallback_id: i64) -> Cow<'static, str> {
        if self.id == 0 {
            return Cow::Borrowed("You");
        }
        if let Some(ref name) = self.full_name {
            let mut result = name.clone();
            if let Some(ref username) = self.username {
                result.push_str(&format!(" (@{})", username));
            }
            return Cow::Owned(result);
        }
        Cow::Owned(format!("User{}", fallback_id))
    }
}

/// 渲染上下文
pub struct RenderContext<'a> {
    /// 账户映射
    accounts: HashMap<i64, &'a Account>,
    /// 话题映射
    topics: HashMap<Uuid, &'a Topic>,
    /// 消息映射
    all_messages: HashMap<i64, &'a Message>,
    /// Bot 账户 ID
    bot_id: i64,
    /// 缓存已格式化的发送者名称
    sender_name_cache: std::cell::RefCell<HashMap<i64, String>>,
    /// 缓存账户信息
    account_info_cache: std::cell::RefCell<HashMap<i64, AccountInfo>>,
    now: Timestamp,
}

impl<'a> RenderContext<'a> {
    /// 创建新的渲染上下文
    pub fn new(
        accounts: &'a [Account],
        topics: &'a [Topic],
        messages: &'a [Message],
        bot_id: i64,
    ) -> Self {
        Self {
            accounts: accounts.iter().map(|a| (a.id, a)).collect(),
            topics: topics.iter().map(|t| (t.id, t)).collect(),
            all_messages: messages.iter().map(|m| (m.id, m)).collect(),
            bot_id,
            sender_name_cache: std::cell::RefCell::new(HashMap::new()),
            account_info_cache: std::cell::RefCell::new(HashMap::new()),
            now: Timestamp::now(),
        }
    }

    /// 获取账户信息（带缓存）
    pub fn get_account_info(&self, account_id: i64) -> Option<AccountInfo> {
        if let Some(info) = self.account_info_cache.borrow().get(&account_id) {
            return Some(info.clone());
        }
        let account = self.accounts.get(&account_id)?;
        let info = AccountInfo::from_account(account);
        self.account_info_cache
            .borrow_mut()
            .insert(account_id, info.clone());
        Some(info)
    }

    /// 获取发送者名称
    pub fn get_sender_name(&self, msg: &Message) -> Cow<'static, str> {
        let account_id = match msg.account_id {
            Some(id) if id == self.bot_id => return Cow::Borrowed("You"),
            Some(id) => id,
            None => {
                return if msg.role == "assistant" {
                    Cow::Borrowed("Other Assistant")
                } else {
                    Cow::Borrowed("User")
                };
            }
        };

        if let Some(name) = self.sender_name_cache.borrow().get(&account_id) {
            return Cow::Owned(name.clone());
        }

        let name = if let Some(info) = self.get_account_info(account_id) {
            info.display_name(account_id).into_owned()
        } else {
            format!("User{} [Unknown Platform]", account_id)
        };

        self.sender_name_cache
            .borrow_mut()
            .insert(account_id, name.clone());
        Cow::Owned(name)
    }

    /// 获取话题标题
    pub fn get_topic_title(&self, msg: &Message) -> Option<String> {
        msg.topic_id
            .and_then(|tid| self.topics.get(&tid))
            .and_then(|t| t.title.clone())
    }

    /// 获取话题提示字符串
    pub fn get_topic_hint(&self, msg: &Message) -> String {
        self.get_topic_title(msg)
            .map(|t| format!(" [T:{}]", t))
            .unwrap_or_default()
    }

    /// 获取话题
    pub fn get_topic(&self, topic_id: Uuid) -> Option<&'a Topic> {
        self.topics.get(&topic_id).copied()
    }

    /// 获取账户
    pub fn get_account(&self, account_id: i64) -> Option<&'a Account> {
        self.accounts.get(&account_id).copied()
    }

    /// 获取消息
    pub fn get_message(&self, message_id: i64) -> Option<&'a Message> {
        self.all_messages.get(&message_id).copied()
    }

    /// 格式化时间戳为本地时区字符串
    pub fn format_sent_at(&self, ts: impl Into<Option<Timestamp>>) -> Cow<'_, str> {
        let Some(ts) = ts.into() else {
            return Cow::Borrowed("None");
        };
        if (self.now - ts).get_days() < 1 {
            return Cow::Owned(format_relative_time(ts));
        }
        Cow::Owned(format!(
            "{} ({})",
            ts.to_zoned(TimeZone::system()),
            format_relative_time(ts)
        ))
    }

    /// 格式化相对时间
    pub fn format_relative_time(&self, ts: impl Into<Timestamp>) -> String {
        format_relative_time(ts.into())
    }

    /// 获取 Bot ID
    pub fn bot_id(&self) -> i64 {
        self.bot_id
    }
}

// =================================================================================
// 辅助函数
// =================================================================================

fn format_relative_time(ts: Timestamp) -> String {
    let now = Timestamp::now();
    let diff = now.duration_since(ts);
    let secs = diff.as_secs();

    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        let m = secs / 60;
        format!("{} minute{} ago", m, if m > 1 { "s" } else { "" })
    } else if secs < 86400 {
        let h = secs / 3600;
        format!("{} hour{} ago", h, if h > 1 { "s" } else { "" })
    } else if secs < 2592000 {
        let d = secs / 86400;
        format!("{} day{} ago", d, if d > 1 { "s" } else { "" })
    } else if secs < 31536000 {
        let mo = secs / 2592000;
        format!("{} month{} ago", mo, if mo > 1 { "s" } else { "" })
    } else {
        let y = secs / 31536000;
        format!("{} year{} ago", y, if y > 1 { "s" } else { "" })
    }
}

fn extract_account_meta(account: &Account) -> (Option<String>, Option<String>) {
    let meta = match account
        .meta
        .as_ref()
        .and_then(|v| <PlatformAccountMeta as serde::Deserialize>::deserialize(v).ok())
    {
        Some(m) => m,
        None => return (None, None),
    };
    (meta.username(), Some(meta.full_name()))
}

impl PlatformAccountMeta {
    pub fn username(&self) -> Option<String> {
        match self {
            PlatformAccountMeta::Telegram(tg) => tg.username.clone(),
        }
    }
}
