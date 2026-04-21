use std::sync::Arc;

use crate::{
    agent::multimodal::EmbeddingService,
    domain::{
        entity::{Account, Chat, Message, Scratchpad, Topic},
        service::{
            MemoryService, MessageService, PlatformService, ScratchpadService, TopicService,
            memory::RelatedMemory,
        },
        vo::{TelegramContentPart, TopicSearchResult},
    },
    error::Result,
    infra::platform::telegram::BotIdentity,
};

/// 消息窗口：限定了范围的消息列表 + 向量检索结果
///
/// 与 RenderContext（渲染上下文/查找表）不同，此结构仅持有原始数据。
#[derive(Debug)]
pub struct MessageWindow {
    pub chat_id: i64,
    /// 当前窗口中的消息列表
    pub messages: Vec<Message>,
    /// 当前窗口中的所有消息 ID（任务完成后标记未读的为已读）
    pub message_ids: Vec<i64>,
    /// 向量检索到的相关记忆（用户事实/知识）
    pub related_memories: Vec<RelatedMemory>,
    /// 向量检索到的相关历史话题（已关闭或活跃的历史话题）
    pub related_topics: Vec<TopicSearchResult>,
}

/// 通用上下文：组装好的纯数据，供展示层消费
///
/// 此结构不包含任何渲染逻辑，渲染职责由 `agent::components::sections::context` 模块承担。
#[derive(Debug)]
pub struct CommonContext {
    /// 当前群聊信息
    pub chat: Chat,
    /// 当前时间
    pub current_time: String,
    /// 当前会话的所有活跃话题
    pub topics: Vec<Topic>,
    /// 消息窗口（含向量检索结果）
    pub messages: MessageWindow,
    /// 涉及到的账号信息
    pub accounts: Vec<Account>,
    /// Bot 自身身份
    pub bot: BotIdentity,
    /// 该 chat 当前 unread 消息的总数（可能大于渲染的窗口大小）
    pub total_unread: i64,
    /// 草稿板（短期工作记忆）
    pub scratchpad: Option<Scratchpad>,
}

// ─── 上下文组装服务 ───────────────────────────────────────────────────────────

/// 上下文组装服务
///
/// 职责：从数据库并发拉取数据并组装为 CommonContext（纯数据）。
/// 不涉及任何渲染逻辑，渲染由 agent 层负责。
pub struct ContextService {
    platform_service: Arc<PlatformService>,
    topic_service: Arc<TopicService>,
    message_service: Arc<MessageService>,
    memory_service: Arc<MemoryService>,
    scratchpad_service: Arc<ScratchpadService>,
    embedding: Arc<EmbeddingService>,
}

impl ContextService {
    pub fn new(
        platform_service: Arc<PlatformService>,
        topic_service: Arc<TopicService>,
        message_service: Arc<MessageService>,
        memory_service: Arc<MemoryService>,
        scratchpad_service: Arc<ScratchpadService>,
        embedding: Arc<EmbeddingService>,
    ) -> Self {
        Self {
            platform_service,
            topic_service,
            message_service,
            memory_service,
            scratchpad_service,
            embedding,
        }
    }

    /// 组装上下文（纯数据）
    pub async fn build_context(
        &self,
        bot: BotIdentity,
        chat_id: i64,
        limit: i64,
    ) -> Result<CommonContext> {
        // 1. 获取基础数据（并发拉取 chat、topic、消息和 scratchpad）
        let chat = self
            .platform_service
            .get_chat_by_id(chat_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Chat not found: {chat_id}"))?;

        let (topics, (messages, total_unread), scratchpad) = tokio::try_join!(
            self.topic_service.get_active_topics(chat_id),
            self.message_service
                .get_messages_for_context(chat_id, limit),
            self.scratchpad_service.get(chat_id),
        )?;

        // 2. 加载窗口外的 reply_to 消息（确保被引用的消息可渲染）
        let reply_context = self.load_reply_context(&messages).await?;

        // 3. 向量检索相关记忆和历史话题（并发执行）
        let (related_memories, related_topics) = self
            .search_related_context(chat_id, &topics, &messages)
            .await?;

        // 4. 收集涉及账号（含 identity 扩展）& 附属记忆
        let accounts = self.collect_accounts_and_ids(&messages).await?;

        // 合并 reply_context 到消息列表，按时间重新排序保证顺序正确
        let mut all_messages = messages;
        all_messages.extend(reply_context);
        all_messages.sort_by(|a, b| {
            a.active_at_sqlx()
                .cmp(&b.active_at_sqlx())
                .then(a.id.cmp(&b.id))
        });

        let message_ids: Vec<i64> = all_messages.iter().map(|m| m.id).collect();

        Ok(CommonContext {
            chat,
            current_time: jiff::Zoned::now().to_string(),
            topics,
            messages: MessageWindow {
                chat_id,
                messages: all_messages,
                message_ids,
                related_memories,
                related_topics,
            },
            accounts,
            bot,
            total_unread,
            scratchpad,
        })
    }

    // ── 内部数据组装 ─────────────────────────────────────────────────────

    /// 加载窗口外被引用消息（reply_to 目标）
    ///
    /// 当消息引用了不在当前窗口中的消息时，从 DB 批量加载这些被引用消息，
    /// 确保渲染时能正确展示 reply_to 内容。
    async fn load_reply_context(&self, messages: &[Message]) -> Result<Vec<Message>> {
        let main_ids: std::collections::HashSet<i64> = messages.iter().map(|m| m.id).collect();
        let missing_reply_ids: Vec<i64> = messages
            .iter()
            .filter_map(|m| m.reply_to_id)
            .filter(|rid| !main_ids.contains(rid))
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        if missing_reply_ids.is_empty() {
            return Ok(Vec::new());
        }

        self.message_service
            .get_messages_by_ids(&missing_reply_ids)
            .await
            .map_err(crate::error::AppError::from)
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
        // ⚠️ TOKEN 消耗标记: 搜索查询拼接了所有话题标题/摘要和消息文本
        // 当话题数多或消息长时，拼接后的 query 会很长，增加 embedding 调用的 token 消耗
        // 考虑截断或采样策略以控制成本
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

        let vector =
            pgvector::Vector::from(self.embedding.generate_embedding(&search_query).await?);

        let (memories, related_topics) = tokio::try_join!(
            self.memory_service
                .search_related_memories(chat_id, &vector, 5),
            self.topic_service.search_topics(chat_id, &vector, 3),
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

    /// 收集账号及其扩展（同一身份下的其他平台账号）
    ///
    /// 返回：(扩展后的 Account 列表, 扩展后的 account ID 集合)
    async fn collect_accounts_and_ids(&self, messages: &[Message]) -> Result<Vec<Account>> {
        let raw_ids: std::collections::HashSet<i64> =
            messages.iter().filter_map(|m| m.account_id).collect();

        let mut account_map: std::collections::HashMap<i64, Account> =
            std::collections::HashMap::new();

        for &id in &raw_ids {
            // 避免重复查询已处理的账号
            if account_map.contains_key(&id) {
                continue;
            }

            if let Some(account) = self.platform_service.get_account_by_id(id).await? {
                if let Some(identity_id) = account.identity_id {
                    let siblings = self
                        .platform_service
                        .get_identity_accounts(identity_id)
                        .await?;
                    for s in siblings {
                        account_map.insert(s.id, s);
                    }
                } else {
                    account_map.insert(id, account);
                }
            }
        }

        Ok(account_map.into_values().collect())
    }
}
