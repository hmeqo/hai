use std::{collections::HashMap, path::PathBuf, str::FromStr};

use genai::chat::ReasoningEffort;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use struct_patch::Patch;
use strum::{EnumIter, EnumString, IntoEnumIterator, IntoStaticStr};

use crate::{
    agentcore::ProviderKind,
    config::{Paths, meta::AGENT_NAME},
    error::{AppResultExt, ErrorKind, Result},
};

// ═══════════════════════════════════════════════════════════════════════════════
// Patchable Config Structs
//   有 `#[derive(Patch)]`，支持热加载时的字段级增量更新。
//   规则：嵌套在另一 Patch struct 下的子 struct，也应加 Patch。
// ═══════════════════════════════════════════════════════════════════════════════

// ── Agent ──

/// 人格等级。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PersonalityTier {
    Low,
    Mid,
    High,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct PersonalityConfig {
    pub name: String,
    pub sociability: PersonalityTier,
    pub verbosity: PersonalityTier,
    pub honesty: PersonalityTier,
    pub humor: PersonalityTier,
    pub rationality: PersonalityTier,
    pub mood: PersonalityTier,
    pub interests: Vec<String>,
    /// 主色调
    pub tone: String,
}

impl Default for PersonalityConfig {
    fn default() -> Self {
        Self {
            name: AGENT_NAME.into(),
            sociability: PersonalityTier::Low,
            verbosity: PersonalityTier::Mid,
            honesty: PersonalityTier::Mid,
            humor: PersonalityTier::High,
            rationality: PersonalityTier::Mid,
            mood: PersonalityTier::Low,
            interests: vec![],
            tone: "你说话不太啰嗦，一句能说清楚的事不会拆成三句。你习惯像跟朋友聊天那样自然、随意。你不爱总结别人说过的话，也不爱分析别人——你知道什么时候该接话、什么时候看看就好。你不太会刻意迎合谁，你有自己的主见。".into(),
        }
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
    pub max_tokens: Option<u32>,
    pub reasoning: bool,
    pub reasoning_effort: String,
    pub temperature: f32,

    pub context: ContextConfig,
    pub personality: PersonalityConfig,
    pub attention: AttentionConfig,
}

impl AgentConfig {
    pub fn reasoning_effort(&self) -> Option<ReasoningEffort> {
        let r = ReasoningEffort::from_keyword(&self.reasoning_effort);
        if r.is_none() {
            tracing::warn!("Invalid reasoning_effort '{}'", self.reasoning_effort);
        }
        r
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct ContextConfig {
    pub system_prompt: String,
    /// 群聊专用 prompt（追加在 system_prompt 之后）
    pub group_prompt: String,
    /// 私聊专用 prompt（追加在 system_prompt 之后）
    pub private_prompt: String,
    pub message_history_limit: i64,
    /// 单次 round 最多加载的消息数
    pub history_cap: i64,
    pub related_memory_limit: i64,
    pub related_topic_limit: i64,
    /// 话题闲置时间（小时），超过此时间的话题标记为 need-close
    pub topic_idle_hours: i64,
    /// 会话空闲超时（秒），窗口关闭后超过此时无活动则重建 session
    pub session_idle_timeout_secs: u64,
    /// 新消息是否可以插队到当前 processing 的下一轮 Turn（默认开启）
    pub preempt: bool,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            system_prompt: String::new(),
            group_prompt:
                "这是群聊。不要每句话都接，大部分时候你只需要看着。\n别人聊的话题你插不上嘴就别硬接，跟你没关系的事保持安静就好。\n消息是有发送对象的，看清楚是发给谁的。没点你名就别自作多情。\n除非对方明确问到你、或者你真的有别人不太可能想到的话要说，否则不开口。\n不要替别人回答问题，别人问的不是你。\n别人换话题了你也别追着旧话题跑，过去了就过去了。\n没什么好说的就别说。"
                    .into(),
            private_prompt: String::new(),
            message_history_limit: 10,
            history_cap: 100,
            related_memory_limit: 5,
            related_topic_limit: 3,
            topic_idle_hours: 3,
            session_idle_timeout_secs: 300,
            preempt: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct AttentionConfig {
    /// 基础注意力（0-1）。agent idle 时随机关注 chat 的概率。
    pub base_attention: f64,
    /// 注意力窗口（秒）。被 @ 或回复后保持高响应的时间。
    pub window_secs: f64,
}

impl Default for AttentionConfig {
    fn default() -> Self {
        Self {
            base_attention: 0.05,
            window_secs: 30.0,
        }
    }
}

// ── Multimodal ──

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
    /// 向量维度，用于 pgvector 列定义（例如 1024、1536）。为空时 rebuild 使用默认值。
    #[serde(default)]
    pub dimension: Option<i32>,
}

impl EmbeddingConfig {
    pub fn provider_or(&self, fallback: &str) -> String {
        self.provider.as_deref().unwrap_or(fallback).to_owned()
    }
    pub fn model(&self) -> String {
        self.model.as_deref().unwrap_or("").to_owned()
    }
    pub fn dimension(&self) -> i32 {
        self.dimension.unwrap_or(1024)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct TtsConfig {
    /// provider 名称（对应 AppConfig.providers 中的 key），为空时使用 agent.provider
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// 模型名称
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

// ═══════════════════════════════════════════════════════════════════════════════
// Non-Patchable Config Structs
//   无 `Patch`，热加载时只能整体替换。
//   理由：整体替换即可满足需求，或属于启动时一次性配置。
// ═══════════════════════════════════════════════════════════════════════════════

/// Bot 平台原始配置（反序列化用，需后续解析为 BotConfig）
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct BotConfigRaw {
    #[serde(rename = "type")]
    pub bot_type: Option<String>,
    pub bot_token: Option<String>,
    pub allowed_chat_ids: Option<Vec<i64>>,
    /// 启用 sendRichMessage
    // #[serde(default = "default_true")]
    pub rich_message: Option<bool>,
}

impl ProviderConfig {
    /// 解析 provider 类型。如果配置了 `type` 则用它，否则从名称推断。
    pub fn infer_kind(&self, name: &str) -> Result<ProviderKind> {
        match self.r#type {
            Some(v) => Ok(v),
            None => ProviderKind::from_str(name)
                .err_kind_msg(ErrorKind::Config, format!("Invalid provider type '{name}'")),
        }
    }
}

/// 单个 provider 的配置
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ProviderConfig {
    /// provider 类型，如 openai, anthropic, requesty 等
    /// 如果不提供，则默认使用配置中的 key 名称
    pub r#type: Option<ProviderKind>,
    /// API key（Ollama 等本地服务可省略）
    pub api_key: Option<String>,
    /// 可选的 base_url 覆盖值
    pub base_url: Option<String>,
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    strum::Display,
    IntoStaticStr,
    EnumString,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum ContainerRuntime {
    Docker,
    Podman,
}

impl ContainerRuntime {
    pub fn as_str(&self) -> &'static str {
        (*self).into()
    }

    fn detect() -> Self {
        if which::which("podman").is_ok() {
            Self::Podman
        } else {
            Self::Docker
        }
    }
}

impl Default for ContainerRuntime {
    fn default() -> Self {
        Self::detect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct SandboxConfig {
    pub enabled: bool,
    pub runtime: ContainerRuntime,
    pub image: String,
    pub timeout_secs: u64,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            runtime: ContainerRuntime::default(),
            image: "ubuntu:latest".into(),
            timeout_secs: 30,
        }
    }
}

/// Skills 配置
///
/// 指定扫描 SKILL.md 的目录列表，支持全局目录和项目本地目录。
/// 示例 hai.toml：
/// ```toml
/// [skills]
/// dirs = ["~/.config/hai/skills", ".hai/skills"]
/// disabled = ["skill-name"]
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SkillsConfig {
    pub dirs: Vec<PathBuf>,
    /// 禁用的 skill 名称列表，不在 discovery 和工具中展示
    #[serde(default)]
    pub disabled: Vec<String>,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            dirs: Paths::inferred().skill_dirs().to_vec(),
            disabled: Vec::new(),
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

/// 解析后的 bot 配置
#[derive(Debug, Clone)]
pub struct BotConfig {
    pub key: String,
    pub platform: BotPlatform,
    pub bot_token: String,
    pub allowed_chat_ids: Vec<i64>,
    pub rich_message: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, EnumIter, EnumString, IntoStaticStr)]
#[strum(serialize_all = "lowercase")]
pub enum BotPlatform {
    Telegram,
}

impl BotPlatform {
    pub fn supported_types() -> Vec<&'static str> {
        Self::iter().map(Into::into).collect()
    }

    pub fn label(&self) -> &'static str {
        self.into()
    }
}

impl BotConfig {
    /// 从 key + raw 解析出配置，未指定 type 时根据 key 名推断
    pub fn resolve(key: &str, raw: &BotConfigRaw) -> Result<Self> {
        let platform = Self::resolve_platform(key, raw)?;
        Ok(Self {
            key: key.to_string(),
            platform,
            bot_token: raw.bot_token.clone().unwrap_or_default(),
            allowed_chat_ids: raw.allowed_chat_ids.clone().unwrap_or_default(),
            rich_message: raw.rich_message.unwrap_or(true),
        })
    }

    fn resolve_platform(key: &str, raw: &BotConfigRaw) -> Result<BotPlatform> {
        let typ = raw.bot_type.as_deref().unwrap_or(key);
        BotPlatform::from_str(typ).err_kind_msg(
            ErrorKind::Config,
            format!(
                "Unknown bot type '{typ}, supported: {}",
                BotPlatform::supported_types().join(", ")
            ),
        )
    }
}
