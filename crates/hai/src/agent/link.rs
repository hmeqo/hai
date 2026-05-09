use std::{fmt::Debug, sync::Arc};

use tokio::sync::mpsc;
use uuid::Uuid;

use super::event::WakeEvent;
use crate::error::Result;

/// 唯一标识一个 bot 实例
#[derive(Debug, Clone, PartialEq, Eq, Hash, derive_more::Display)]
pub struct BotId(pub Arc<str>);

impl BotId {
    pub fn new(s: impl AsRef<str>) -> Self {
        Self(Arc::from(s.as_ref()))
    }
}

/// Bot 身份信息（平台无关，仅供 agent 上下文渲染使用）
#[derive(Debug, Clone)]
pub struct BotProfile {
    pub account_id: i64,
    pub username: String,
    pub name: String,
}

// ─── 发送请求 / 响应类型 ──────────────────────────────────────────────────────

#[derive(Debug)]
pub struct SendMessageReq {
    pub chat_id: i64,
    pub content: String,
    pub topic_id: Option<Uuid>,
    pub platform_reply_to_id: Option<i64>,
}

#[derive(Debug)]
pub struct SendVoiceReq {
    pub chat_id: i64,
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

// ─── PlatformHandler ──────────────────────────────────────────────────────────

/// 平台无关的 bot 能力抽象，由各平台实现。
///
/// 涵盖消息发送、输入指示、以及附件文件获取。
/// agent 层只依赖此 trait，不感知具体平台。
#[async_trait::async_trait]
pub trait PlatformHandler: Debug + Send + Sync + 'static {
    /// 发送文本消息
    async fn send_message(&self, req: SendMessageReq) -> Result<SentMessageMeta>;
    /// 发送语音消息
    async fn send_voice(&self, req: SendVoiceReq) -> Result<SentMessageMeta>;
    /// 发送"正在输入"指示（fire-and-forget）
    async fn send_typing(&self, chat_id: i64);
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
}

// ─── BotConn ─────────────────────────────────────────────────────────────────

/// Agent 侧持有的 bot 连接（封装身份 + 平台处理器）
#[derive(Clone)]
pub struct BotConn {
    pub bot_id: BotId,
    pub profile: BotProfile,
    pub handler: Arc<dyn PlatformHandler>,
}

impl BotConn {
    pub fn new(bot_id: BotId, profile: BotProfile, handler: Arc<dyn PlatformHandler>) -> Self {
        Self {
            bot_id,
            profile,
            handler,
        }
    }

    pub async fn send_message(&self, req: SendMessageReq) -> Result<SentMessageMeta> {
        self.handler.send_message(req).await
    }

    pub async fn send_voice(&self, req: SendVoiceReq) -> Result<SentMessageMeta> {
        self.handler.send_voice(req).await
    }

    pub async fn send_typing(&self, chat_id: i64) {
        self.handler.send_typing(chat_id).await;
    }

    /// 获取文件内容（委托给平台 handler）
    pub async fn download_file(&self, file_id: &str) -> Result<Vec<u8>> {
        self.handler.download_file(file_id).await
    }

    /// 获取文件 URL（委托给平台 handler）
    pub async fn get_file_url(&self, file_id: &str) -> Result<String> {
        self.handler.get_file_url(file_id).await
    }
}

// ─── Link ────────────────────────────────────────────────────────────────────

/// Bot 侧的连接半体，持有向 agent 发事件的 sender
pub struct BotLink {
    pub bot_id: BotId,
    pub event_tx: mpsc::UnboundedSender<WakeEvent>,
}

/// Agent 侧的连接半体，持有接收 bot 事件的 receiver
pub struct AgentLink {
    pub bot_id: BotId,
    pub event_rx: mpsc::UnboundedReceiver<WakeEvent>,
}

/// 建立一对 (BotLink, AgentLink)
pub fn open_link(bot_id: BotId) -> (BotLink, AgentLink) {
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    (
        BotLink {
            bot_id: bot_id.clone(),
            event_tx,
        },
        AgentLink { bot_id, event_rx },
    )
}
