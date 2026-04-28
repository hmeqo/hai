use std::{collections::HashMap, path::PathBuf, str::FromStr, sync::Arc};

use autoagents::llm::chat::ReasoningEffort;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use struct_patch::Patch;

use crate::{
    agentcore::provider::ProviderBackend,
    config::{PathResolver, meta::AGENT_NAME},
    error::{ErrorKind, Result},
};

// =============================================================================
// 场景提示词默认值（供 ContextConfig 和 agent::prompts 共同使用）
// =============================================================================

/// 群聊场景层
pub const GROUP_SCENE_PROMPT: &str = r#"## 群聊场景
你在这个群潜水很久了。你打开群看了眼消息，做该做的后台整理，没什么事就走了。
你大多数时候都不说话——这就是你的日常。"#;

/// 私聊场景层
pub const PRIVATE_SCENE_PROMPT: &str = r#"## 私聊场景
- 积极响应用户的每条消息
"#;

/// 人格配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct PersonalityConfig {
    pub name: String,
    /// 社交活跃度：0=沉默潜水，1=活跃话痨
    pub sociability: f64,
    /// 话量：0=极简，1=详尽
    pub verbosity: f64,
    /// 坦诚度：0=圆滑世故，1=真诚诚实
    pub honesty: f64,
    /// 幽默感：0=严肃正经，1=风趣幽默
    pub humor: f64,
    /// 理性/感性：0=绝对理性，1=感性丰富
    pub rationality: f64,
    /// 情绪：0=情绪稳定，1=情绪外露
    pub mood: f64,
    pub interests: Vec<String>,
    /// 主色调，如"你是一个友善的人"
    pub tone: String,
    pub communication_style: String,
}

impl Default for PersonalityConfig {
    fn default() -> Self {
        Self {
            name: AGENT_NAME.into(),
            sociability: 0.05,
            verbosity: 0.35,
            honesty: 0.65,
            humor: 0.70,
            rationality: 0.75,
            mood: 0.1,
            interests: vec![],
            tone: "你是一个友善的人".into(),
            communication_style: "口语化，网络聊天风格".into(),
        }
    }
}

impl PersonalityConfig {
    pub fn dims(&self) -> Vec<(&str, f64, &str)> {
        vec![
            ("Sociability", self.sociability, "沉默潜水 ←→ 活跃话痨"),
            ("Verbosity", self.verbosity, "极简 ←→ 详尽"),
            ("Honesty", self.honesty, "圆滑世故 ←→ 真诚诚实"),
            ("Humor", self.humor, "严肃正经 ←→ 风趣幽默"),
            ("Rationality", self.rationality, "绝对理性 ←→ 感性丰富"),
            ("Mood", self.mood, "情绪稳定 ←→ 情绪外露"),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch, Default)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct AgentConfig {
    /// 当前使用的 provider 名称（对应 AppConfig.providers 中的 key）
    pub provider: String,
    pub model: String,
    pub max_tokens: u32,
    pub reasoning: bool,
    pub reasoning_effort: String,
    pub temperature: f32,

    pub context: ContextConfig,
    pub personality: PersonalityConfig,
    pub trigger: TriggerConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct ContextConfig {
    pub system_prompt: String,
    pub group_prompt: String,
    pub private_prompt: String,
    pub sliding_window_size: usize,
    pub message_history_limit: i64,
    pub related_memory_limit: i64,
    pub related_topic_limit: i64,
    pub format_context: bool,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            system_prompt: String::new(),
            group_prompt: GROUP_SCENE_PROMPT.into(),
            private_prompt: PRIVATE_SCENE_PROMPT.into(),
            message_history_limit: 10,
            sliding_window_size: 10,
            related_memory_limit: 5,
            related_topic_limit: 3,
            format_context: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct TriggerConfig {
    /// 群聊触发器的最小热度（基础响应概率）
    pub min_heat: f64,
    /// 最小热度上限（由 sociability 决定的 min_heat 不能超过这个值，防止过于频繁）
    pub min_heat_cap: f64,
    pub debounce_ms: u64,
}

impl Default for TriggerConfig {
    fn default() -> Self {
        Self {
            min_heat: 0.02,
            min_heat_cap: 0.33,
            debounce_ms: 1000,
        }
    }
}

/// 解析后的 provider 信息（包含 backend、base_url、api_key）
#[derive(Debug, Clone)]
pub struct ResolvedProvider {
    pub name: String,
    pub config: Arc<ProviderConfig>,
    pub backend: ProviderBackend,
    pub base_url: String,
}

impl ResolvedProvider {
    /// 获取有效的 type（优先使用配置中的 type，否则使用 key 名称）
    pub fn effective_type(&self) -> &str {
        self.config.r#type.as_deref().unwrap_or(&self.name)
    }

    pub fn base_url(&self) -> &str {
        self.config
            .base_url
            .as_deref()
            .unwrap_or_else(|| self.backend.default_base_url())
    }

    pub fn override_base_url(&self) -> Option<&str> {
        self.config.base_url.as_deref()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct TelegramConfig {
    pub bot_token: String,
    pub allowed_chat_ids: Vec<i64>,
}

/// Skills 配置
///
/// 指定扫描 SKILL.md 的目录列表，支持全局目录和项目本地目录。
/// 示例 hai.toml：
/// ```toml
/// [skills]
/// dirs = ["~/.config/hai/skills", ".hai/skills"]
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SkillsConfig {
    pub dirs: Vec<PathBuf>,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            dirs: PathResolver::skill_dirs(),
        }
    }
}

/// TTS 语音合成配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct TtsConfig {
    /// provider 名称（对应 AppConfig.providers 中的 key），为空时使用 agent.provider
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// 模型名称，为空时使用 "tts-1"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// 发音人
    pub voice: String,
    /// 语速，0.25 ~ 4.0
    pub speed: f32,
    enabled: bool,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            provider: None,
            model: None,
            enabled: true,
            voice: "alloy".into(),
            speed: 1.0,
        }
    }
}

impl TtsConfig {
    pub fn enabled(&self) -> bool {
        self.model.is_some() && self.enabled
    }
}

/// 单个多模态基础配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct MultimodalSubConfig {
    /// provider 名称（对应 AppConfig.providers 中的 key），为空时使用 agent.provider
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// 模型名称
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    enabled: bool,
    /// 该类别的默认分析提示词（空值时由应用使用内置默认值）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_prompt: Option<String>,
}

impl Default for MultimodalSubConfig {
    fn default() -> Self {
        Self {
            provider: None,
            model: None,
            enabled: true,
            default_prompt: None,
        }
    }
}

impl MultimodalSubConfig {
    pub fn enabled(&self) -> bool {
        self.model.is_some() && self.enabled
    }
}

/// 多模态输入配置（附件理解）
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct MultimodalInputConfig {
    /// 图片理解
    pub image: MultimodalSubConfig,
    /// 音频理解
    pub audio: MultimodalSubConfig,
    /// 视频理解
    pub video: MultimodalSubConfig,
}

/// 模态类型
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ModalityType {
    Text,
    Image,
    Audio,
    Video,
}

/// 多模态输出模型配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct GenerationModelConfig {
    pub provider: String,
    pub model: String,
    pub enabled: bool,
    pub input_type: ModalityType,
    pub output_type: ModalityType,
}

impl Default for GenerationModelConfig {
    fn default() -> Self {
        Self {
            provider: String::new(),
            model: String::new(),
            enabled: true,
            input_type: ModalityType::Text,
            output_type: ModalityType::Image,
        }
    }
}

impl GenerationModelConfig {
    pub fn enabled(&self) -> bool {
        self.enabled
    }
}

/// 嵌入模型配置
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct EmbeddingConfig {
    /// provider，为空时使用 agent.provider
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// 模型名称，为空时由 provider 决定
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// 多模态配置
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct MultimodalConfig {
    /// 向量嵌入
    pub embedding: EmbeddingConfig,
    /// 输入（理解）配置
    pub input: MultimodalInputConfig,
    /// 语音合成输出
    pub tts: TtsConfig,
}

/// 单个 provider 的配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProviderConfig {
    /// provider 类型，如 openai, anthropic, requesty 等
    /// 如果不提供，则默认使用配置中的 key 名称
    pub r#type: Option<String>,
    /// API key
    pub api_key: String,
    /// 可选的 base_url 覆盖值
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct McpConfig {
    pub r#type: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Option<HashMap<String, String>>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            r#type: "stdio".into(),
            command: String::new(),
            args: Vec::new(),
            env: None,
        }
    }
}

impl AgentConfig {
    pub fn reasoning_effort(&self) -> Result<ReasoningEffort> {
        match self.reasoning_effort.as_str() {
            "low" => Ok(ReasoningEffort::Low),
            "medium" => Ok(ReasoningEffort::Medium),
            "high" => Ok(ReasoningEffort::High),
            _ => Err(ErrorKind::InvalidParameter.with_msg(format!(
                "Invalid reasoning effort: {}",
                self.reasoning_effort
            ))),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct DatabaseConfig {
    /// PostgreSQL 连接字符串，例如：postgres://user:password@localhost/dbname
    pub url: String,
    /// 连接池最大连接数
    pub max_connections: u32,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            max_connections: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct LoggingConfig {
    pub level: String,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_owned(),
        }
    }
}

impl LoggingConfig {
    pub fn level(&self) -> tracing::Level {
        tracing::Level::from_str(&self.level).unwrap()
    }
}
