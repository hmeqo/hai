use std::{collections::HashMap, path::PathBuf, str::FromStr, sync::Arc};

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use struct_patch::Patch;
use strum::{EnumIter, EnumString, IntoEnumIterator, IntoStaticStr};

use crate::{
    agentcore::provider::ProviderBackend,
    config::{PathResolver, meta::AGENT_NAME},
    error::{AppResultExt, ErrorKind, Result},
};

// ═══════════════════════════════════════════════════════════════════════════════
// Patchable Config Structs
//   有 `#[derive(Patch)]`，支持热加载时的字段级增量更新。
//   规则：嵌套在另一 Patch struct 下的子 struct，也应加 Patch。
// ═══════════════════════════════════════════════════════════════════════════════

// ── Agent ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
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
    /// 主色调
    pub tone: String,
}

impl Default for PersonalityConfig {
    fn default() -> Self {
        Self {
            name: AGENT_NAME.into(),
            sociability: 0.05,
            verbosity: 0.35,
            honesty: 0.60,
            humor: 0.70,
            rationality: 0.35,
            mood: 0.1,
            interests: vec![],
            tone: "你是一个有自己的主见的人, 你很友善很聪明, 你不会太过刻意的表达自己的意见或一味附和别人".into(),
        }
    }
}

impl PersonalityConfig {
    fn curve(t: f64) -> f64 {
        1.0 - (1.0 - t).powf(1.5)
    }

    pub fn base_attention(&self, cfg: &AttentionConfig) -> f64 {
        let base = Self::curve(self.sociability) * cfg.base_attention_cap;
        base.max(cfg.base_attention)
    }

    /// 注意力窗口时长（秒）：agent 主动参与后保持高注意力的时长
    pub fn attention_window_secs(&self) -> f64 {
        5.0 + Self::curve(self.sociability) * 55.0
    }

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
    pub max_tokens: Option<u32>,
    pub reasoning: bool,
    pub reasoning_effort: String,
    pub temperature: f32,

    pub context: ContextConfig,
    pub personality: PersonalityConfig,
    pub attention: AttentionConfig,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

impl AgentConfig {
    pub fn reasoning_effort(&self) -> Result<ReasoningEffort> {
        match self.reasoning_effort.as_str() {
            "low" => Ok(ReasoningEffort::Low),
            "medium" => Ok(ReasoningEffort::Medium),
            "high" => Ok(ReasoningEffort::High),
            _ => Err(ErrorKind::InvalidParameter.msg(format!(
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
    /// 会话模式：ephemeral（单次执行）或 persistent（跨轮次累积）
    pub conversation_mode: ConversationMode,
    /// 会话空闲超时（秒），窗口关闭后超过此时无活动则重建 session
    pub session_idle_timeout_secs: u64,
    /// 新消息是否可以插队到当前 processing 的下一轮 Turn（默认开启）
    pub preempt: bool,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            system_prompt: String::new(),
            group_prompt: "\
群里你就是一个普通群友。

一桌子人在聊天，有想法就接话，没什么特别的要说就听着，这不奇怪。
不用每条消息都回应——现实中也不会这样。
别人聊得正起劲的时候，别硬插进去打断。
"
            .into(),
            private_prompt: String::new(),
            message_history_limit: 10,
            history_cap: 100,
            related_memory_limit: 5,
            related_topic_limit: 3,
            topic_idle_hours: 3,
            conversation_mode: ConversationMode::default(),
            session_idle_timeout_secs: 300,
            preempt: true,
        }
    }
}

/// 会话模式
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, strum::IntoStaticStr)]
#[serde(rename_all = "kebab-case")]
pub enum ConversationMode {
    /// 每次执行使用全新对话，不保留上下文
    Ephemeral,
    /// 跨执行保留上下文累积
    #[default]
    Persistent,
}

impl ConversationMode {
    pub fn label(&self) -> &'static str {
        self.into()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct AttentionConfig {
    /// 基础注意力（agent idle 时关注 chat 的最低概率）
    pub base_attention: f64,
    /// 基础注意力上限（sociability 能推到的最大值，防止过于频繁）
    pub base_attention_cap: f64,
}

impl Default for AttentionConfig {
    fn default() -> Self {
        Self {
            base_attention: 0.02,
            base_attention_cap: 0.33,
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

// ── Misc ──

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
}

/// 单个 provider 的配置
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ProviderConfig {
    /// provider 类型，如 openai, anthropic, requesty 等
    /// 如果不提供，则默认使用配置中的 key 名称
    pub r#type: Option<String>,
    /// API key（Ollama 等本地服务可省略）
    pub api_key: Option<String>,
    /// 可选的 base_url 覆盖值
    pub base_url: Option<String>,
}

#[derive(
    Debug,
    Clone,
    Copy,
    Default,
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
    #[default]
    Docker,
    Podman,
}

impl ContainerRuntime {
    pub fn as_str(&self) -> &'static str {
        (*self).into()
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
            dirs: PathResolver::skill_dirs(),
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

// ═══════════════════════════════════════════════════════════════════════════════
// Internal Derived Types
//   非反序列化配置，由系统运行时从原始配置解析生成。
// ═══════════════════════════════════════════════════════════════════════════════

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

/// 解析后的 bot 配置
#[derive(Debug, Clone)]
pub struct BotConfig {
    pub key: String,
    pub platform: BotPlatform,
    pub bot_token: String,
    pub allowed_chat_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, EnumIter, EnumString, IntoStaticStr)]
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

/// 模态类型
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ModalityType {
    Text,
    Image,
    Audio,
    Video,
}
