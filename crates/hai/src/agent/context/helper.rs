use std::collections::{HashMap, HashSet};

use linkify::LinkFinder;
use uuid::Uuid;

use crate::{
    agent::context::types::{
        Attachment, AttachmentPerceptionMap, ParsedContent, PerceptionResult, SearchResult,
    },
    config::AppConfig,
    domain::{
        model::{Account, Message, Perception, Topic},
        service::DbServices,
        vo::{ChatId, resource_id_from_file_id},
    },
    error::{ErrorKind, OptionAppExt, Result},
};

/// 按 ID 获取聊天记录
pub async fn load_chat(
    services: &DbServices,
    chat_id: ChatId,
) -> Result<crate::domain::model::Chat> {
    services
        .platform
        .get_chat_by_id(chat_id)
        .await?
        .ok_or_err_msg(ErrorKind::NotFound, format!("Chat not found: {chat_id}"))
}

/// 加载消息中引用但尚未在集合中的回复上下文
pub async fn load_reply_context(
    services: &DbServices,
    messages: &[Message],
) -> Result<Vec<Message>> {
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
    services
        .message
        .get_messages_by_ids(
            &missing
                .iter()
                .map(|id| crate::domain::vo::MessageId(*id))
                .collect::<Vec<_>>(),
        )
        .await
}

/// 收集消息中所有 account（含 identity 关联的 sibling account）
pub async fn collect_accounts(services: &DbServices, messages: &[Message]) -> Result<Vec<Account>> {
    let raw_ids: HashSet<i64> = messages.iter().filter_map(|m| m.account_id).collect();
    let mut account_map: HashMap<i64, Account> = HashMap::new();

    for id in raw_ids {
        if account_map.contains_key(&id) {
            continue;
        }
        if let Some(account) = services
            .platform
            .get_account_by_id(crate::domain::vo::AccountId(id))
            .await?
        {
            if let Some(identity_id) = account.identity_id {
                for sibling in services
                    .platform
                    .get_identity_accounts(crate::domain::vo::IdentityId(identity_id))
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

// ── Perception 加载器 ─────────────────────────────────────────────────────────

struct PerceptionLoader<'a> {
    services: &'a DbServices,
    perceptions: Vec<Perception>,
    seen: HashSet<Uuid>,
    by_attachment_id: HashMap<Uuid, Vec<Perception>>,
    same_resource_as: HashMap<Uuid, Uuid>,
}

impl<'a> PerceptionLoader<'a> {
    fn new(services: &'a DbServices) -> Self {
        Self {
            services,
            perceptions: Vec::new(),
            seen: HashSet::new(),
            by_attachment_id: HashMap::new(),
            same_resource_as: HashMap::new(),
        }
    }

    async fn load_file_attachments(&mut self, parts: &[&Attachment]) -> Result<()> {
        let file_ids: Vec<String> = parts.iter().map(|a| a.file_id.clone()).collect();
        let mut file_id_perceptions: HashMap<String, Vec<Perception>> = HashMap::new();
        for (fid, p) in self
            .services
            .perception
            .find_by_platform_file_ids(&file_ids)
            .await?
        {
            file_id_perceptions.entry(fid).or_default().push(p);
        }

        let mut first_file_attachment: HashMap<Uuid, Uuid> = HashMap::new();
        for att in parts {
            let file_uid = resource_id_from_file_id(&att.file_id);
            let hit = file_id_perceptions.get(&att.file_id);

            if let Some(&first) = first_file_attachment.get(&file_uid) {
                self.same_resource_as.insert(att.id, first);
            } else {
                first_file_attachment.insert(file_uid, att.id);
                if let Some(ps) = hit {
                    self.by_attachment_id.insert(att.id, ps.clone());
                }
            }

            for p in hit.into_iter().flatten() {
                if self.seen.insert(p.id) {
                    self.perceptions.push(p.clone());
                }
            }
        }
        Ok(())
    }

    async fn load_urls(&mut self, parsed: &[ParsedContent]) -> Result<()> {
        let urls: Vec<String> = parsed
            .iter()
            .flat_map(|p| p.text_fragments.iter())
            .flat_map(|text| extract_urls(text))
            .collect();

        if !urls.is_empty() {
            let url_perceptions = self.services.perception.find_by_urls(&urls).await?;
            for p in url_perceptions {
                if self.seen.insert(p.id) {
                    self.perceptions.push(p);
                }
            }
        }
        Ok(())
    }

    fn build_attachment_map(self) -> AttachmentPerceptionMap {
        AttachmentPerceptionMap {
            by_attachment_id: self.by_attachment_id,
            same_resource_as: self.same_resource_as,
        }
    }

    fn build_perception_result(self) -> PerceptionResult {
        PerceptionResult {
            items: self.perceptions,
            map: AttachmentPerceptionMap {
                by_attachment_id: self.by_attachment_id,
                same_resource_as: self.same_resource_as,
            },
        }
    }
}

/// 查询附件感知数据（按文件 ID 和 URL）
pub async fn load_perceptions(
    services: &DbServices,
    parsed: &[ParsedContent],
) -> Result<PerceptionResult> {
    let mut loader = PerceptionLoader::new(services);

    let attachment_parts: Vec<&Attachment> =
        parsed.iter().flat_map(|p| p.attachments.iter()).collect();
    if !attachment_parts.is_empty() {
        loader.load_file_attachments(&attachment_parts).await?;
    }
    loader.load_urls(parsed).await?;

    Ok(loader.build_perception_result())
}

/// 仅构建附件映射（不返回感知列表）
pub async fn build_attachment_maps(
    services: &DbServices,
    parser: &dyn crate::agent::context::types::ContentParser,
    messages: &[Message],
) -> Result<AttachmentPerceptionMap> {
    if messages.is_empty() {
        return Ok(AttachmentPerceptionMap {
            by_attachment_id: HashMap::new(),
            same_resource_as: HashMap::new(),
        });
    }
    let parsed: Vec<ParsedContent> = messages.iter().map(|m| parser.parse(&m.content)).collect();
    let mut loader = PerceptionLoader::new(services);

    let attachment_parts: Vec<&Attachment> =
        parsed.iter().flat_map(|p| p.attachments.iter()).collect();
    if !attachment_parts.is_empty() {
        loader.load_file_attachments(&attachment_parts).await?;
    }

    Ok(loader.build_attachment_map())
}

/// 向量搜索相关内容（记忆+话题）
pub async fn search_related_context(
    services: &DbServices,
    cfg: &AppConfig,
    chat_id: ChatId,
    topics: &[Topic],
    parsed: &[ParsedContent],
    perceptions: &[Perception],
) -> Result<SearchResult> {
    let search_query: String = topics
        .iter()
        .flat_map(|t| [t.title.clone(), t.summary.clone()])
        .flatten()
        .chain(parsed.iter().map(|p| p.text.clone()))
        .chain(perceptions.iter().map(|p| p.content.clone()))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if search_query.is_empty() {
        return Ok(SearchResult {
            memories: Vec::new(),
            topics: Vec::new(),
        });
    }

    let embedding = services
        .multimodal
        .generate_embedding(&search_query)
        .await?;
    let ctx_cfg = &cfg.agent.context;
    let (memories, mut related_topics) = match tokio::try_join!(
        services
            .memory
            .search_related(chat_id, &embedding, ctx_cfg.related_memory_limit),
        services
            .topic
            .search_related_topics(chat_id, &embedding, ctx_cfg.related_topic_limit),
    ) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(%chat_id, "Vector search failed (try clearing old embeddings?): {e}");
            (Vec::new(), Vec::new())
        }
    };

    let active_ids: HashSet<Uuid> = topics.iter().map(|t| t.id).collect();
    related_topics.retain(|r| !active_ids.contains(&r.topic.id));
    Ok(SearchResult {
        memories,
        topics: related_topics,
    })
}

/// 检索相关内容，排除已展示项。后续轮自动按 2/3 缩减（5→3, 3→2）。
pub async fn search_related_dedup(
    services: &DbServices,
    cfg: &AppConfig,
    chat_id: ChatId,
    topics: &[Topic],
    parsed: &[ParsedContent],
    perceptions: &[Perception],
    shown_memory_ids: &HashSet<Uuid>,
    shown_topic_ids: &HashSet<Uuid>,
) -> Result<SearchResult> {
    let SearchResult { memories, topics } =
        search_related_context(services, cfg, chat_id, topics, parsed, perceptions).await?;

    // 首轮（空 set）全量返回
    if shown_memory_ids.is_empty() && shown_topic_ids.is_empty() {
        return Ok(SearchResult { memories, topics });
    }

    // 后续轮：过滤已展示，按原有 limit 的 2/3 缩减
    let cfg = &cfg.agent.context;
    let ml = (cfg.related_memory_limit * 2 / 3) as usize;
    let tl = (cfg.related_topic_limit * 2 / 3) as usize;

    Ok(SearchResult {
        memories: memories
            .into_iter()
            .filter(|m| !shown_memory_ids.contains(&m.id.0))
            .take(ml)
            .collect(),
        topics: topics
            .into_iter()
            .filter(|t| !shown_topic_ids.contains(&t.topic.id))
            .take(tl)
            .collect(),
    })
}

fn extract_urls(text: &str) -> Vec<String> {
    let mut finder = LinkFinder::new();
    finder
        .kinds(&[linkify::LinkKind::Url])
        .links(text)
        .map(|l| l.as_str().to_string())
        .collect()
}
