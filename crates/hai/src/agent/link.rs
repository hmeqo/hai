use std::sync::Arc;

use tokio::sync::mpsc;
use uuid::Uuid;

use super::event::AgentEvent;
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

// ─── BotSender ───────────────────────────────────────────────────────────────

/// 平台无关的 bot 发送能力抽象，由各平台 actor 实现
#[async_trait::async_trait]
pub trait BotSender: Send + Sync + 'static {
    async fn send_message(&self, req: SendMessageReq) -> Result<SentMessageMeta>;
    async fn send_voice(&self, req: SendVoiceReq) -> Result<SentMessageMeta>;
    fn send_typing(&self, chat_id: i64);
}

// ─── BotConn ─────────────────────────────────────────────────────────────────

/// Agent 侧持有的 bot 连接（封装身份 + 平台发送器）
#[derive(Clone)]
pub struct BotConn {
    pub bot_id: BotId,
    pub profile: BotProfile,
    sender: Arc<dyn BotSender>,
}

impl BotConn {
    pub fn new(bot_id: BotId, profile: BotProfile, sender: Arc<dyn BotSender>) -> Self {
        Self {
            bot_id,
            profile,
            sender,
        }
    }

    pub async fn send_message(&self, req: SendMessageReq) -> Result<SentMessageMeta> {
        self.sender.send_message(req).await
    }

    pub async fn send_voice(&self, req: SendVoiceReq) -> Result<SentMessageMeta> {
        self.sender.send_voice(req).await
    }

    pub fn send_typing(&self, chat_id: i64) {
        self.sender.send_typing(chat_id);
    }
}

// ─── Link ────────────────────────────────────────────────────────────────────

/// Bot 侧的连接半体，持有向 agent 发事件的 sender
pub struct BotLink {
    pub bot_id: BotId,
    pub event_tx: mpsc::UnboundedSender<AgentEvent>,
}

/// Agent 侧的连接半体，持有接收 bot 事件的 receiver
pub struct AgentLink {
    pub bot_id: BotId,
    pub event_rx: mpsc::UnboundedReceiver<AgentEvent>,
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
