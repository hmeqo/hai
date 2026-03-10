use std::sync::Arc;

use anyhow::Result;
use uuid::Uuid;

use crate::{
    agent::{
        multimodal::EmbeddingService,
        render::{
            render_chat_info, render_conversation_log, render_involved_accounts_section,
            render_memory_section, render_related_memories_section, render_related_topics_section,
            render_topic_section,
        },
    },
    app::{
        domain::{
            entity::{Account, Chat, Memory, Message, Topic},
            model::{TelegramContentPart, TopicSearchResult},
        },
        service::{
            MemoryService, MessageService, PlatformService, TopicService, memory::RelatedMemory,
        },
    },
};

// ─── 封装结构 ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct MessageContext {
    pub chat_id: i64,
    /// 当前上下文中的消息列表
    pub messages: Vec<Message>,
    /// 向量检索到的相关记忆（用户事实/知识）
    pub related_memories: Vec<RelatedMemory>,
    /// 向量检索到的相关历史话题（已关闭或活跃的历史话题）
    pub related_topics: Vec<TopicSearchResult>,
}

#[derive(Debug)]
pub struct CommonContext {
    /// 当前群聊信息
    pub chat: Chat,
    /// 当前时间
    pub current_time: String,
    /// 当前会话的所有活跃话题
    pub topics: Vec<Topic>,
    /// 消息上下文
    pub messages: MessageContext,
    /// 涉及到的账号信息
    pub accounts: Vec<Account>,
    /// 相关的记忆（笔记、规则等）
    pub memories: Vec<Memory>,
    /// 当前 AI 的账号 ID
    pub bot_account_id: i64,
    /// 该 chat 当前 pending 消息的总数（可能大于渲染的窗口大小）
    pub total_pending: i64,
}

impl CommonContext {
    pub fn render(&self, bot_account_id: i64) -> String {
        let mut s = String::new();

        s.push_str("## Context Information\n");
        s.push_str(&format!("- Current Time: {}\n", self.current_time));

        // 告知 agent 还有多少未读（超出渲染窗口的部分）
        let shown_pending = self
            .messages
            .messages
            .iter()
            .filter(|m| m.interaction_status == "pending")
            .count() as i64;
        let remaining = self.total_pending - shown_pending;
        if remaining > 0 {
            s.push_str(&format!(
                "- Unread Messages: showing {shown_pending} of {} total pending (use `get_history_messages` to fetch more)\n",
                self.total_pending
            ));
        } else {
            s.push_str(&format!("- Unread Messages: {shown_pending} pending\n"));
        }
        s.push('\n');

        s.push_str(&render_chat_info(&self.chat));
        s.push_str(&render_involved_accounts_section(&self.accounts));
        s.push_str(&render_topic_section(&self.topics));
        s.push_str(&render_related_topics_section(
            &self.messages.related_topics,
        ));
        s.push_str(&render_related_memories_section(
            &self.messages.related_memories,
        ));
        s.push_str(&render_memory_section(&self.memories));

        // 使用对话流格式渲染消息
        s.push_str(&render_conversation_log(
            &self.messages.messages,
            &self.accounts,
            &self.topics,
            bot_account_id,
        ));

        s
    }
}

// ─── Hai 上下文 ───────────────────────────────────────────────────────────────

/// Hai 所需的上下文
#[derive(Debug)]
pub struct HaiContext {
    pub common: CommonContext,
}

impl HaiContext {
    pub fn render_as_prompt(&self, instruction: &str) -> String {
        let mut s = self.common.render(self.common.bot_account_id);
        s.push_str("\n## Task Instructions\n");
        s.push_str(instruction);
        s.push('\n');
        s
    }
}

// ─── 上下文组装服务 ───────────────────────────────────────────────────────────

pub struct ContextService {
    embedding: Arc<EmbeddingService>,
    platform_service: Arc<PlatformService>,
    topic_service: Arc<TopicService>,
    message_service: Arc<MessageService>,
    memory_service: Arc<MemoryService>,
}

impl ContextService {
    pub fn new(
        embedding: Arc<EmbeddingService>,
        platform_service: Arc<PlatformService>,
        topic_service: Arc<TopicService>,
        message_service: Arc<MessageService>,
        memory_service: Arc<MemoryService>,
    ) -> Self {
        Self {
            embedding,
            platform_service,
            topic_service,
            message_service,
            memory_service,
        }
    }

    /// 组装通用上下文：
    /// 1. 并发拉取 chat、topic 列表、消息窗口
    /// 2. 向量检索相关记忆 & 相关历史话题（并发）
    /// 3. 收集涉及账号 & 附属记忆
    async fn build_common_context(
        &self,
        bot_account_id: i64,
        chat_id: i64,
        limit: i64,
    ) -> Result<CommonContext> {
        // 1. 获取基础数据（并发拉取 topic 和消息）
        let chat = self
            .platform_service
            .get_chat_by_id(chat_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Chat not found: {chat_id}"))?;

        let (topics, (messages, total_pending)) = tokio::try_join!(
            self.topic_service.get_active_topics(chat_id),
            self.message_service.get_messages(chat_id, limit),
        )?;

        // 2. 向量检索相关记忆和历史话题（并发执行）
        let (related_memories, related_topics) = self
            .search_related_context(chat_id, &topics, &messages)
            .await?;

        let message_ctx = MessageContext {
            chat_id,
            messages,
            related_memories,
            related_topics,
        };

        // 3. 收集涉及账号 & 附属记忆
        let accounts = self.collect_accounts(&message_ctx).await?;
        let memories = self.fetch_memories(&topics, &message_ctx).await?;

        Ok(CommonContext {
            chat,
            current_time: jiff::Zoned::now().to_string(),
            topics,
            messages: message_ctx,
            accounts,
            memories,
            bot_account_id,
            total_pending,
        })
    }

    /// 并发向量检索相关记忆和相关历史话题
    ///
    /// 搜索文本来自：话题标题/摘要 + 消息内容
    /// 记忆检索跨平台身份共享（同一用户在不同平台的记忆也被检索）
    async fn search_related_context(
        &self,
        chat_id: i64,
        topics: &[Topic],
        messages: &[Message],
    ) -> Result<(Vec<RelatedMemory>, Vec<TopicSearchResult>)> {
        // 1. 构建搜索查询文本
        let search_query: String = topics
            .iter()
            .flat_map(|t| [t.title.as_deref(), t.summary.as_deref()])
            .flatten()
            .map(|s| s.to_string())
            .chain(
                messages
                    .iter()
                    .map(|m| TelegramContentPart::extract_text_from_value(&m.content)),
            )
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");

        if search_query.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        // 2. 生成查询向量
        let vector =
            pgvector::Vector::from(self.embedding.generate_embedding(&search_query).await?);

        // 3. 并发：检索相关记忆（跨平台身份）和相关历史话题
        let account_ids = self.collect_account_ids_with_identity(messages).await?;

        let (memories, related_topics) = tokio::try_join!(
            self.memory_service
                .search_related_memories(&account_ids, Some(chat_id), &vector, 5),
            self.topic_service.search_topics(chat_id, &vector, 5),
        )?;

        // 过滤掉已在 active topics 中的话题（避免重复）
        let active_topic_ids: std::collections::HashSet<uuid::Uuid> =
            topics.iter().map(|t| t.id).collect();
        let related_topics = related_topics
            .into_iter()
            .filter(|r| !active_topic_ids.contains(&r.topic.id))
            .collect();

        Ok((memories, related_topics))
    }

    /// 从消息列表中收集所有账号 ID，并将同一身份下的其他平台账号也并入（用于跨平台记忆共享）
    async fn collect_account_ids_with_identity(&self, messages: &[Message]) -> Result<Vec<i64>> {
        // 提取消息中涉及的账号 ID
        let raw_ids: std::collections::HashSet<i64> =
            messages.iter().filter_map(|m| m.account_id).collect();

        // 跨平台身份扩展
        let mut all_ids = raw_ids.clone();
        for id in raw_ids {
            if let Some(account) = self.platform_service.get_account_by_id(id).await? {
                if let Some(identity_id) = account.identity_id {
                    let siblings = self
                        .platform_service
                        .get_identity_accounts(identity_id)
                        .await?;
                    all_ids.extend(siblings.iter().map(|s| s.id));
                }
            }
        }
        Ok(all_ids.into_iter().collect())
    }

    async fn collect_accounts(&self, messages: &MessageContext) -> Result<Vec<Account>> {
        let mut account_ids = std::collections::HashSet::new();
        for m in &messages.messages {
            if let Some(aid) = m.account_id {
                account_ids.insert(aid);
            }
        }

        let mut accounts = Vec::new();
        for id in account_ids {
            if let Some(account) = self.platform_service.get_account_by_id(id).await? {
                accounts.push(account);
            }
        }
        Ok(accounts)
    }

    async fn fetch_memories(&self, topics: &[Topic], ctx: &MessageContext) -> Result<Vec<Memory>> {
        let mut memories = Vec::new();

        // 1. 获取群聊规则
        if let Some(rule) = self.memory_service.get_chat_rule(ctx.chat_id).await? {
            memories.push(rule);
        }

        // 2. 获取话题相关的记忆
        let mut topic_ids: Vec<Uuid> = topics.iter().map(|t| t.id).collect();
        for m in &ctx.messages {
            if let Some(tid) = m.topic_id {
                if !topic_ids.contains(&tid) {
                    topic_ids.push(tid);
                }
            }
        }
        if !topic_ids.is_empty() {
            let topic_memories = self
                .memory_service
                .get_memories_by_topics(&topic_ids)
                .await?;
            memories.extend(topic_memories);
        }

        // 3. 获取消息相关的记忆
        let message_ids: Vec<i64> = ctx.messages.iter().map(|m| m.id).collect();
        if !message_ids.is_empty() {
            let message_memories = self
                .memory_service
                .get_memories_by_messages(&message_ids)
                .await?;
            memories.extend(message_memories);
        }

        Ok(memories)
    }

    pub async fn build_hai_context(
        &self,
        bot_account_id: i64,
        chat_id: i64,
        limit: i64,
    ) -> Result<HaiContext> {
        let common = self
            .build_common_context(bot_account_id, chat_id, limit)
            .await?;
        Ok(HaiContext { common })
    }
}
