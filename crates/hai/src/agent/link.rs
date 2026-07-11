use std::fmt::{self, Debug};

use genai::chat::ChatMessage;
use uuid::Uuid;

pub use crate::agent::context::ContentParser;
use crate::{config::schema::BotPlatform, domain::vo::ChatId, error::Result};

/// 平台构建好的上下文，供 agent 直接使用
#[derive(Debug)]
pub struct BuiltContext {
    /// 渲染好的 prompt 字符串
    pub rendered_prompt: String,
    /// 分段消息列表（[System] + [User]）
    pub messages: Vec<ChatMessage>,
    /// 需要标记为已读的消息 ID 列表
    pub message_ids: Vec<i64>,
    /// 本轮已展示的记忆和话题 ID，用于后续轮 dedup
    pub shown_memory_ids: Vec<Uuid>,
    pub shown_topic_ids: Vec<Uuid>,
}

/// Bot 实例标识，按平台区分。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BotId {
    Telegram { key: String },
}

impl BotId {
    pub fn new(key: String, platform: BotPlatform) -> Self {
        match platform {
            BotPlatform::Telegram => BotId::Telegram { key },
        }
    }

    pub fn key(&self) -> &str {
        match self {
            BotId::Telegram { key } => key,
        }
    }
}

impl fmt::Display for BotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BotId::Telegram { key } => write!(f, "telegram:{key}"),
        }
    }
}

/// Bot 身份信息（仅供 agent 上下文渲染使用）
#[derive(Debug, Clone)]
pub struct BotProfile {
    pub account_id: i64,
    pub username: String,
    pub name: String,
}

// ─── 发送请求 / 响应类型 ──────────────────────────────────────────────────────

#[derive(Debug)]
pub struct SendMessageReq {
    pub chat_id: ChatId,
    pub content: String,
    pub topic_id: Option<Uuid>,
    pub platform_reply_to_id: Option<i64>,
}

#[derive(Debug)]
pub struct SendVoiceReq {
    pub chat_id: ChatId,
    pub audio_bytes: Vec<u8>,
    pub prompt: String,
    pub topic_id: Option<Uuid>,
    pub platform_reply_to_id: Option<i64>,
}

/// 发送后的平台元数据
#[derive(Debug, Clone)]
pub struct SentMessageMeta {
    /// 平台侧消息 ID（如 Telegram message_id）
    pub external_id: String,
}

// ─── PlatformHandler ─────────────────────────────────────────────────────────

/// 平台消息格式能力。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageCapability {
    Rich,
    MarkdownV2,
    Plain,
}

/// 平台无关的 bot 能力抽象，由各平台实现。
#[async_trait::async_trait]
pub trait PlatformHandler: Debug + Send + Sync + 'static {
    /// 平台标识
    fn bot_id(&self) -> BotId;
    /// Bot 身份信息（name + username + account_id）
    fn profile(&self) -> BotProfile;
    /// 发送消息
    async fn send_message(&self, req: SendMessageReq) -> Result<SentMessageMeta>;
    /// 发送语音消息
    async fn send_voice(&self, req: SendVoiceReq) -> Result<SentMessageMeta>;
    /// 发送"正在输入"指示（fire-and-forget）
    async fn send_typing(&self, chat_id: ChatId);
    /// 下载文件内容（用于附件分析）
    async fn download_file(&self, file_id: &str) -> Result<Vec<u8>>;
    /// 获取文件的可公开访问 URL（用于多模态分析，避免下载大文件）
    async fn get_file_url(&self, file_id: &str) -> Result<String>;
    /// 分析消息附件（由各平台根据自身格式实现）
    async fn analyze_attachment(
        &self,
        attachment_uuid: Uuid,
        prompt: Option<&str>,
    ) -> Result<String>;
    /// 平台消息解析器
    fn content_parser(&self) -> &'static dyn ContentParser;
    /// 平台消息格式能力（用于 tool description 差异化描述）
    fn message_capability(&self) -> MessageCapability;
}
