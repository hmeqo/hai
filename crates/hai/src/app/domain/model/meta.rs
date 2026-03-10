use serde::{Deserialize, Serialize};

/// Agent 消息元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessageMeta {
    pub model: String,
}

/// 平台账号元数据（方便扩展多平台）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "platform")]
pub enum PlatformAccountMeta {
    Telegram(TelegramAccountMeta),
}

/// Telegram 账号元数据
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelegramAccountMeta {
    pub first_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

impl PlatformAccountMeta {
    /// 获取完整姓名
    pub fn full_name(&self) -> String {
        match self {
            PlatformAccountMeta::Telegram(v) => match &v.last_name {
                Some(last) => format!("{} {}", v.first_name, last),
                None => v.first_name.clone(),
            },
        }
    }
}

/// 大模型消息元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmMessageMeta {
    pub model: String,
    /// 大模型 reasoning 内容
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// 解析后的内容（如语音转文字、图片 OCR 等）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parsed_content: Option<String>,
}

/// 消息元数据（顶层结构，包含通用字段和平台特定字段）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageMeta {
    /// 平台特定信息
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub platform: Option<PlatformMessageMeta>,
    /// 大模型信息（如果是 AI 消息）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub llm: Option<LlmMessageMeta>,
}

/// 平台消息元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "platform")]
pub enum PlatformMessageMeta {
    Telegram(TelegramMessageMeta),
}

/// Telegram 消息元数据
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelegramMessageMeta {
    /// 消息是否来自话题
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_thread_id: Option<i32>,
}
