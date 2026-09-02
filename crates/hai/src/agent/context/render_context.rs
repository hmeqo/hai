use std::{collections::HashMap, sync::Arc};

use derive_more::Deref;

use crate::{
    agent::{
        context::{ContextMessages, fmt::display_name},
        link::BotProfile,
    },
    agentcore::render::elements::Node,
    domain::{
        model::{Account, Chat, Message, Perception, Topic},
        service::{knowledge::RelatedChunk, memory::RelatedMemory},
        vo::TopicSearchResult,
    },
};

/// 消息内容渲染函数。各平台注入自己的实现，section builder 通过此函数渲染消息体。
pub type ContentRenderer = Arc<dyn Fn(&serde_json::Value) -> Vec<Node> + Send + Sync>;

// ─── 主结构 ──────────────────────────────────────────────────────────────────

/// 一次 agent 运行的完整上下文快照 + 渲染策略
#[derive(Deref)]
pub struct RenderContext {
    #[deref]
    pub data: RenderContextData,

    /// 消息内容渲染函数（各平台注入）
    pub content_renderer: ContentRenderer,

    // ── 内置索引（构建时一次性建立，O(1) 查询）──────────────────────────
    accounts_by_id: HashMap<i64, usize>,
}

/// 构建 `RenderContext` 所需的输入数据（平台无关）
#[derive(Debug)]
pub struct RenderContextData {
    // ── 身份与环境 ──────────────────────────────────────────────────────
    pub bot: BotProfile,
    pub chat: Chat,
    pub current_time: String,
    pub total_unread: i64,

    // ── 消息 ─────────────────────────────────────────────────────────────────
    /// 上下文消息：对话流（窗口）+ 窗口外引用上下文（`ContextMessages::get` 带来源）
    pub messages: ContextMessages,

    // ── 参与者 ────────────────────────────────────────────────────────────────
    pub accounts: Vec<Account>,

    // ── 话题 ──────────────────────────────────────────────────────────────────
    pub topics: Vec<Topic>,
    pub related_topics: Vec<TopicSearchResult>,

    // ── 记忆 ──────────────────────────────────────────────────────────────────
    pub related_memories: Vec<RelatedMemory>,

    // ── 知识库 ────────────────────────────────────────────────────────────────
    pub related_knowledge: Vec<RelatedChunk>,

    // ── 感知 ──────────────────────────────────────────────────────────────────
    pub perceptions: Vec<Perception>,

    // ── 配置 ──────────────────────────────────────────────────────────────────
    /// 话题闲置超时（小时），超过此时间的话题标记为 need-close
    pub topic_idle_hours: i64,
}

impl RenderContext {
    pub fn new(data: RenderContextData, content_renderer: ContentRenderer) -> Self {
        let mut ctx = Self {
            accounts_by_id: HashMap::new(),
            data,
            content_renderer,
        };
        ctx.build_indices();
        ctx
    }

    fn build_indices(&mut self) {
        for (i, account) in self.data.accounts.iter().enumerate() {
            self.accounts_by_id.insert(account.id, i);
        }
    }

    pub fn get_account(&self, id: i64) -> Option<&Account> {
        self.accounts_by_id
            .get(&id)
            .copied()
            .and_then(|i| self.data.accounts.get(i))
    }

    pub fn sender_name(&self, msg: &Message) -> String {
        let aid = msg.account_id.unwrap_or(0);

        // Bot 账号直接用 BotProfile（name + username），不依赖 Account 表 meta
        if aid == self.bot.account_id {
            let n = &self.bot;
            return if n.username.is_empty() {
                n.name.clone()
            } else {
                format!("{} (@{})", n.name, n.username)
            };
        }

        display_name(self.get_account(aid), aid)
    }

    pub fn perceptions(&self) -> &[Perception] {
        &self.data.perceptions
    }
}
