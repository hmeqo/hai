use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use uuid::Uuid;

use super::render_context::{RenderContext, RenderContextData};
use crate::{
    bot::telegram::BotIdentity,
    config::AppConfig,
    domain::{
        entity::{Message, Perception},
        service::DbServices,
        vo::{TelegramContentPart, resource_id_from_file_id},
    },
    error::{ErrorKind, OptionAppExt, Result},
};

/// 上下文组装服务
#[derive(Clone)]
pub struct ContextFactory {
    config: Arc<AppConfig>,
    services: DbServices,
}

impl ContextFactory {
    pub fn new(config: Arc<AppConfig>, services: DbServices) -> Self {
        Self { config, services }
    }

    pub async fn build_context(
        &self,
        bot: BotIdentity,
        chat_id: i64,
        limit: i64,
    ) -> Result<RenderContext> {
        let chat = self
            .services
            .platform
            .get_chat_by_id(chat_id)
            .await?
            .ok_or_err_msg(ErrorKind::NotFound, format!("Chat not found: {chat_id}"))?;

        let (topics, (messages, total_unread), scratchpad) = tokio::try_join!(
            self.services.topic.get_active_topics(chat_id),
            self.services
                .message
                .get_messages_for_context(chat_id, limit),
            self.services.scratchpad.get(chat_id),
        )?;

        let reply_context = self.load_reply_context(&messages).await?;
        let mut all_messages = messages;
        all_messages.extend(reply_context);
        all_messages.sort_by(|a, b| {
            a.active_at_sqlx()
                .cmp(&b.active_at_sqlx())
                .then(a.id.cmp(&b.id))
        });
        let message_ids: Vec<i64> = all_messages.iter().map(|m| m.id).collect();

        let (perceptions, perception_by_attachment_id, same_resource_as) =
            self.load_perceptions(&all_messages).await?;

        let (related_memories, related_topics) = self
            .search_related_context(chat_id, &topics, &all_messages, &perceptions)
            .await?;
        let accounts = self.collect_accounts(&all_messages).await?;

        Ok(RenderContext::new(RenderContextData {
            bot,
            chat,
            current_time: jiff::Zoned::now().to_string(),
            messages: all_messages,
            message_ids,
            total_unread,
            topics,
            related_topics,
            related_memories,
            accounts,
            perceptions,
            perception_by_attachment_id,
            same_resource_as,
            scratchpad,
        }))
    }

    async fn load_reply_context(&self, messages: &[Message]) -> Result<Vec<Message>> {
        let main_ids: HashSet<i64> = messages.iter().map(|m| m.id).collect();
        let missing: Vec<i64> = messages
            .iter()
            .filter_map(|m| m.reply_to_id)
            .filter(|rid| !main_ids.contains(rid))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        if missing.is_empty() {
            return Ok(Vec::new());
        }
        self.services.message.get_messages_by_ids(&missing).await
    }

    async fn search_related_context(
        &self,
        chat_id: i64,
        topics: &[crate::domain::entity::Topic],
        messages: &[Message],
        perceptions: &[Perception],
    ) -> Result<(
        Vec<crate::domain::service::memory::RelatedMemory>,
        Vec<crate::domain::vo::TopicSearchResult>,
    )> {
        let search_query: String = topics
            .iter()
            .flat_map(|t| [t.title.clone(), t.summary.clone()])
            .flatten()
            .chain(
                messages
                    .iter()
                    .map(|m| TelegramContentPart::extract_text_from_value(&m.content)),
            )
            .chain(perceptions.iter().map(|p| p.content.clone()))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");

        if search_query.is_empty() {
            return Ok((Vec::new(), Vec::new()));
        }

        let vector = pgvector::Vector::from(
            self.services
                .multimodal
                .generate_embedding(&search_query)
                .await?,
        );
        let ctx_cfg = &self.config.agent.context;
        let (memories, mut related_topics) = tokio::try_join!(
            self.services.memory.search_related_memories(
                chat_id,
                &vector,
                ctx_cfg.related_memory_limit
            ),
            self.services.topic.search_related_topics(
                chat_id,
                &vector,
                ctx_cfg.related_topic_limit
            ),
        )?;

        let active_ids: HashSet<Uuid> = topics.iter().map(|t| t.id).collect();
        related_topics.retain(|r| !active_ids.contains(&r.topic.id));

        Ok((memories, related_topics))
    }

    async fn collect_accounts(
        &self,
        messages: &[Message],
    ) -> Result<Vec<crate::domain::entity::Account>> {
        let raw_ids: HashSet<i64> = messages.iter().filter_map(|m| m.account_id).collect();
        let mut account_map: HashMap<i64, crate::domain::entity::Account> = HashMap::new();

        for id in raw_ids {
            if account_map.contains_key(&id) {
                continue;
            }
            if let Some(account) = self.services.platform.get_account_by_id(id).await? {
                if let Some(identity_id) = account.identity_id {
                    for sibling in self
                        .services
                        .platform
                        .get_identity_accounts(identity_id)
                        .await?
                    {
                        account_map.insert(sibling.id, sibling);
                    }
                } else {
                    account_map.insert(id, account);
                }
            }
        }

        Ok(account_map.into_values().collect())
    }

    /// 加载窗口内所有 attachment 和 URL 的 perception 结果。
    ///
    /// 流程：收集消息中的 (attachment_id, file_id) → 查 resource 表拿到 resource_id →
    /// 查 perception 表拿到分析结果 → 构建 attachment_id → Perception 映射。
    ///
    /// 返回 (所有去重 perception, attachment 内嵌映射, 重复 resource 指引映射)。
    async fn load_perceptions(
        &self,
        messages: &[Message],
    ) -> Result<(
        Vec<Perception>,
        HashMap<Uuid, Vec<Perception>>,
        HashMap<Uuid, Uuid>,
    )> {
        let mut perceptions: Vec<Perception> = Vec::new();
        let mut seen: HashSet<Uuid> = HashSet::new();
        let mut perception_by_attachment_id: HashMap<Uuid, Vec<Perception>> = HashMap::new();
        let mut same_resource_as: HashMap<Uuid, Uuid> = HashMap::new();

        // ── 1. 收集消息中所有附件的 (attachment_id, file_id) ──────────────
        let attachment_parts: Vec<(Uuid, String)> = messages
            .iter()
            .filter_map(|m| {
                serde_json::from_value::<Vec<TelegramContentPart>>(m.content.clone()).ok()
            })
            .flatten()
            .filter_map(|part| {
                let aid = part.attachment_id()?;
                let fid = part.file_id()?.to_string();
                Some((aid, fid))
            })
            .collect();

        if !attachment_parts.is_empty() {
            // ── 2. 按 file_id 批量查 perception ─────────────────────────
            let file_ids: Vec<String> = attachment_parts
                .iter()
                .map(|(_, fid)| fid.clone())
                .collect();
            let mut file_id_perceptions: HashMap<String, Vec<Perception>> = HashMap::new();
            for (fid, p) in self
                .services
                .perception
                .find_by_platform_file_ids(&file_ids)
                .await?
            {
                file_id_perceptions.entry(fid).or_default().push(p);
            }

            // ── 3. 构建 attachment 映射 ─────────────────────────────────
            // 同一 file_id 的多个 attachment：首个内嵌全部 <analysis>，
            // 后续的标记 same_resource_as 指向首个，避免重复。
            // 用确定性 UUID 做文件间去重。
            let mut first_file_attachment: HashMap<Uuid, Uuid> = HashMap::new();
            for (att_id, fid) in &attachment_parts {
                let file_uid = resource_id_from_file_id(fid);
                let hit = file_id_perceptions.get(fid);

                if let Some(&first) = first_file_attachment.get(&file_uid) {
                    same_resource_as.insert(*att_id, first);
                } else {
                    first_file_attachment.insert(file_uid, *att_id);
                    if let Some(ps) = hit {
                        perception_by_attachment_id.insert(*att_id, ps.clone());
                    }
                }

                for p in hit.into_iter().flatten() {
                    if seen.insert(p.id) {
                        perceptions.push(p.clone());
                    }
                }
            }
        }

        // ── 5. 从消息文本中提取 URL ──────────────────────────────────────
        let urls: Vec<String> = messages
            .iter()
            .filter_map(|m| {
                serde_json::from_value::<Vec<TelegramContentPart>>(m.content.clone()).ok()
            })
            .flatten()
            .filter_map(|part| match part {
                TelegramContentPart::Text { text } => Some(text),
                _ => part.text().map(|s| s.to_string()),
            })
            .flat_map(|text| extract_urls(&text))
            .collect();

        if !urls.is_empty() {
            let url_perceptions = self.services.perception.find_by_urls(&urls).await?;
            for p in url_perceptions {
                if seen.insert(p.id) {
                    perceptions.push(p);
                }
            }
        }

        Ok((perceptions, perception_by_attachment_id, same_resource_as))
    }
}

/// 从文本中提取 http/https URL
fn extract_urls(text: &str) -> Vec<String> {
    let mut finder = linkify::LinkFinder::new();
    finder
        .kinds(&[linkify::LinkKind::Url])
        .links(text)
        .map(|l| l.as_str().to_string())
        .collect()
}
