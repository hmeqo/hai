use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use jiff::Timestamp;
use jiff::tz::TimeZone;
use uuid::Uuid;

use crate::agent::render::content;
use crate::app::domain::entity::{Account, Chat, Memory, Message, MessageStatus, Topic};
use crate::app::domain::model::{PlatformAccountMeta, TopicSearchResult};
use crate::app::service::memory::RelatedMemory;

// =================================================================================
// 1. Context: 将分散的数据打包，并提供简单的查询逻辑
// =================================================================================

struct RenderContext<'a> {
    accounts: HashMap<i64, &'a Account>,
    topics: HashMap<Uuid, &'a Topic>,
    all_messages: HashMap<i64, &'a Message>,
    bot_id: i64,
    // 缓存已格式化的发送者名称，避免重复解析元数据
    sender_name_cache: std::cell::RefCell<HashMap<i64, String>>,
}

impl<'a> RenderContext<'a> {
    fn new(
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
        }
    }

    /// 获取发送者名称（核心逻辑封装）
    fn get_sender_name(&self, msg: &Message) -> String {
        if msg.account_id == Some(self.bot_id) {
            return "You".to_string();
        }

        let account_id = match msg.account_id {
            Some(id) => id,
            None => {
                return if msg.role == "assistant" {
                    "Other Assistant".to_string()
                } else {
                    "User".to_string()
                };
            }
        };

        // 检查缓存
        if let Some(name) = self.sender_name_cache.borrow().get(&account_id) {
            return name.clone();
        }

        // 尝试查找账户并格式化
        let name = if let Some(account) = self.accounts.get(&account_id) {
            let name = extract_full_name(account).unwrap_or_else(|| format!("User{}", account_id));
            // username 现在在 meta 中，extract_full_name 已经处理了基本名称
            // 如果需要显示平台特定的 username，可以从 meta 中提取
            let username = extract_username(account)
                .map(|u| format!(" (@{})", u))
                .unwrap_or_default();
            format!("{}{}[{}]", name, username, account.platform)
        } else {
            format!("User{}[Unknown Platform]", account_id)
        };

        // 存入缓存
        self.sender_name_cache
            .borrow_mut()
            .insert(account_id, name.clone());
        name
    }

    /// 获取话题提示
    fn get_topic_hint(&self, msg: &Message) -> String {
        msg.topic_id
            .and_then(|tid| self.topics.get(&tid))
            .map(|t| format!(" [Topic:{}]", t.title.as_deref().unwrap_or("No Title")))
            .unwrap_or_default()
    }
}

// =================================================================================
// 2. 渲染主逻辑: 线性、清晰、无深层嵌套
// =================================================================================

pub fn render_conversation_log(
    messages: &[Message],
    accounts: &[Account],
    topics: &[Topic],
    bot_account_id: i64,
) -> String {
    if messages.is_empty() {
        return String::new();
    }

    // 建立主消息 ID 集合
    let main_ids: HashSet<i64> = messages.iter().map(|m| m.id).collect();

    // 收集所有被 reply-to 但不在主消息列表中的消息，作为引用上下文渲染在头部
    let ctx = RenderContext::new(accounts, topics, messages, bot_account_id);
    let mut output = String::with_capacity(messages.len() * 200);
    let mut rendered_ids = HashSet::new();

    // ── 引用上下文：在窗口外的 reply 消息，预先展示完整内容供 LLM 理解 ──
    // 用 HashSet<i64> 对 reply_id 去重，避免 Message 未实现 Hash/Eq 的问题
    let mut seen_reply_ids = HashSet::<i64>::new();
    let reply_context_msgs: Vec<&Message> = messages
        .iter()
        .filter_map(|m| m.reply_to_id)
        .filter(|rid| !main_ids.contains(rid) && seen_reply_ids.insert(*rid))
        .filter_map(|rid| ctx.all_messages.get(&rid).copied())
        .collect();

    if !reply_context_msgs.is_empty() {
        output.push_str("## Reply Context (out-of-window referenced messages)\n");
        // 按 id 排序保持稳定顺序
        let mut sorted = reply_context_msgs;
        sorted.sort_by_key(|m| m.id);
        for msg in sorted {
            render_single_message(&ctx, msg, &mut rendered_ids, &mut output);
        }
        output.push('\n');
    }

    output.push_str("## Chat History\n");

    let first_unread_idx = messages
        .iter()
        .position(|m| m.interaction_status == MessageStatus::Pending.as_str());

    if let Some(idx) = first_unread_idx {
        if idx > 0 {
            for msg in &messages[..idx] {
                render_single_message(&ctx, msg, &mut rendered_ids, &mut output);
            }
            output.push('\n');
        }

        output.push_str("### New Messages\n");

        for msg in &messages[idx..] {
            render_single_message(&ctx, msg, &mut rendered_ids, &mut output);
        }
    } else {
        for msg in messages {
            render_single_message(&ctx, msg, &mut rendered_ids, &mut output);
        }
    }

    output.push('\n');
    output
}

/// 渲染单条消息的逻辑
fn render_single_message(
    ctx: &RenderContext,
    msg: &Message,
    rendered_ids: &mut HashSet<i64>,
    output: &mut String,
) {
    // 如果消息已渲染，跳过
    if !rendered_ids.insert(msg.id) {
        return;
    }

    let sender_name = ctx.get_sender_name(msg);
    let topic_hint = ctx.get_topic_hint(msg);

    let pid_hint = msg
        .external_id
        .as_ref()
        .map(|id| format!(" [PlatformID:{}]", id))
        .unwrap_or_default();
    let time_str = msg
        .sent_at()
        .map(|t| {
            format!(
                "{} ({})",
                t.to_zoned(TimeZone::system()),
                format_relative_time(t)
            )
        })
        .unwrap_or_else(|| "None".to_string());

    // 使用 write! 宏直接写入 output，避免中间 String 分配
    let _ = writeln!(
        output,
        "- [ID:{}]{} [{}]\n  SENDER: {} [R:{}] [S:{}]",
        msg.id, pid_hint, time_str, sender_name, msg.role, msg.interaction_status,
    );

    if !topic_hint.is_empty() {
        let _ = writeln!(output, "  TOPIC:{}", topic_hint);
    }

    // 渲染内容
    let content_str = render_message_content(msg);
    let _ = writeln!(output, "  ```content\n{}\n  ```", content_str);

    // reply-to：只记录 ID，完整内容已在 Reply Context 段或主消息列表中展示
    if let Some(reply_id) = msg.reply_to_id {
        let _ = writeln!(output, "  > Reply to [ID:{}]", reply_id);
    }
}

// =================================================================================
// 3. 其他辅助渲染函数
// =================================================================================

pub fn render_chat_info(chat: &Chat) -> String {
    let mut s = String::with_capacity(256);
    let _ = writeln!(
        s,
        "### Current Chat\n- ID: {}\n- Platform: {}\n- Type: {}\n- Created At: {}",
        chat.id,
        chat.platform,
        chat.chat_type,
        chat.created_at()
    );
    if let Some(name) = &chat.name {
        let _ = writeln!(s, "- Name: {}", name);
    }
    s.push('\n');
    s
}

pub fn render_account_info(account: &Account) -> String {
    let mut s = String::with_capacity(128);
    render_account_info_to(account, &mut s);
    s
}

pub fn render_account_info_to(account: &Account, s: &mut String) {
    let _ = write!(s, "- [ID:{}] [{}]", account.id, account.platform);

    if let Some(username) = extract_username(account) {
        let _ = write!(s, " @{}", username);
    }
    if let Some(name) = extract_full_name(account) {
        let _ = write!(s, " ({})", name);
    }
    if let Some(iid) = account.identity_id {
        let _ = write!(s, " [IdentityID:{}]", iid);
    }
    s.push('\n');
}

pub fn render_topic_section(topics: &[Topic]) -> String {
    if topics.is_empty() {
        return String::new();
    }

    let mut s = String::with_capacity(topics.len() * 150);
    s.push_str("### Topics\n");
    let now = Timestamp::now();
    let tz = TimeZone::system();

    for topic in topics {
        let is_inactive = now.duration_since(topic.last_active_at()).as_secs() > 6 * 3600;
        let inactive_tag = if is_inactive && topic.status == "active" {
            " [Inactive]"
        } else {
            ""
        };

        let started_at = format!(
            "{} ({})",
            topic.started_at().to_zoned(tz.clone()),
            format_relative_time(topic.started_at())
        );
        let last_active_at = format!(
            "{} ({})",
            topic.last_active_at().to_zoned(tz.clone()),
            format_relative_time(topic.last_active_at())
        );

        let _ = writeln!(
            s,
            "- [ID:{}] Title: {} | Status: {}{} | Started At: {} | Last Active: {}\n  Summary: {}",
            topic.id,
            topic.title.as_deref().unwrap_or("No Title"),
            topic.status,
            inactive_tag,
            started_at,
            last_active_at,
            topic.summary.as_deref().unwrap_or("No Summary")
        );
    }
    s.push('\n');
    s
}

fn render_list_section<T, F>(title: &str, items: &[T], formatter: F) -> String
where
    F: Fn(&T, &mut String),
{
    if items.is_empty() {
        return String::new();
    }

    let mut s = format!("### {}\n", title);
    for item in items {
        formatter(item, &mut s);
    }
    s.push('\n');
    s
}

pub fn render_related_memories_section(memories: &[RelatedMemory]) -> String {
    render_list_section("Related Memories", memories, |mem, s| {
        let source = mem
            .account_id
            .map(|id| format!("UserID:{}", id))
            .unwrap_or_else(|| "System".into());
        let _ = writeln!(
            s,
            "- [Source:{}] [Relevance:{:.4}] {}",
            source, mem.distance, mem.content
        );
    })
}

pub fn render_related_topics_section(topics: &[TopicSearchResult]) -> String {
    render_list_section("Related History Topics", topics, |r, s| {
        let _ = writeln!(
            s,
            "- [ID:{}] [Relevance:{:.4}] {} | {}",
            r.topic.id,
            r.distance,
            r.topic.title.as_deref().unwrap_or("No Title"),
            r.topic.summary.as_deref().unwrap_or("No Summary"),
        );
    })
}

pub fn render_memory_section(memories: &[Memory]) -> String {
    render_list_section("Knowledge & Rules", memories, |mem, s| {
        let _ = writeln!(
            s,
            "- [Type:{}] [Created At:{}] {}",
            mem.type_,
            mem.created_at(),
            mem.content
        );
    })
}

pub fn render_involved_accounts_section(accounts: &[Account]) -> String {
    render_list_section("Participant Information", accounts, |acc, s| {
        render_account_info_to(acc, s);
    })
}

// ---------------- Helper ----------------

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

fn extract_full_name(account: &Account) -> Option<String> {
    account
        .meta
        .as_ref()
        .and_then(|v| <PlatformAccountMeta as serde::Deserialize>::deserialize(v).ok())
        .map(|m| m.full_name())
}

fn extract_username(account: &Account) -> Option<String> {
    account
        .meta
        .as_ref()
        .and_then(|v| <PlatformAccountMeta as serde::Deserialize>::deserialize(v).ok())
        .and_then(|m| match m {
            PlatformAccountMeta::Telegram(tg) => tg.username,
        })
}

fn render_message_content(msg: &Message) -> String {
    content::render_content_from_value(&msg.content)
}
