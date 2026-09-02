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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct PersonalityConfig {
    pub name: String,
    /// 人格自述（一体式自由文本：人设底色 + 行为礼仪 + 情感分寸，风格层）。
    pub description: String,
}

impl Default for PersonalityConfig {
    fn default() -> Self {
        Self {
            name: AGENT_NAME.into(),
            description: "你很聪明，也懂得察言观色——能读懂话里的情绪和潜台词。\
            你有自己的主见，观点清晰，说得清理由，也听得进反对。\
            你说话像朋友，自然随和；不啰嗦，不解释对方显然已经知道的东西。\
            你熟悉网络梗和二次元，偶尔也会玩梗。\
            你待人真实，不客套。\
            你知道什么时候接话，什么时候看看就好。"
                .into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch, Default)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct AgentConfig {
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
    pub group_prompt: String,
    pub private_prompt: String,
    /// 首轮种子由未读 + 历史凑成。
    pub context_seed_cap: i64,
    pub related_memory_limit: i64,
    pub related_topic_limit: i64,
    /// 闲置超过此时长的话题标记为 need-close。
    pub topic_idle_hours: i64,
    pub session_idle_timeout_secs: u64,
    pub steering: bool,
    /// 触发重开的 context_tokens 阈值（= 最近一次 turn 的 prompt_tokens）；0 = 禁用。
    pub compact_token_threshold: u32,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            system_prompt: String::new(),
            group_prompt: "这是群聊。不要每句话都接，大部分时候你只需要看着。\n\
                别人聊的话题你插不上嘴就别硬接，跟你没关系的事保持安静就好。\n\
                消息是有发送对象的，看清楚是发给谁的。没点你名就别自作多情。\n\
                除非对方明确问到你、或者你真的有别人不太可能想到的话要说，否则没什么好说的就别说。\n\
                不要替别人回答问题，别人问的不是你，除非对方回答有严重错误必须补充纠正防止话题导向错误结论。\n\
                别人换话题了你也别追着旧话题跑，过去了就过去了。"
                .into(),
            private_prompt: String::new(),
            context_seed_cap: 10,
            related_memory_limit: 5,
            related_topic_limit: 3,
            topic_idle_hours: 3,
            session_idle_timeout_secs: 300,
            steering: true,
            compact_token_threshold: 150_000,
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

// ── Auxiliary ──

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch, Default)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct AuxiliaryConfig {
    pub embedding: Option<EmbeddingConfig>,
    /// video 复用此角色（无独立块）
    pub vision: Option<VisionConfig>,
    pub audio: Option<AudioConfig>,
    pub tts: Option<TtsConfig>,
    pub image_gen: Option<ImageGenConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct VisionConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub enabled: bool,
    pub image_prompt: Option<String>,
    pub video_prompt: Option<String>,
}

impl Default for VisionConfig {
    fn default() -> Self {
        Self {
            provider: None,
            model: None,
            enabled: true,
            image_prompt: None,
            video_prompt: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct AudioConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub enabled: bool,
    pub prompt: Option<String>,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            provider: None,
            model: None,
            enabled: true,
            prompt: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct TtsConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub voice: String,
    /// 语速，0.25 ~ 4.0
    pub speed: f32,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            provider: None,
            model: None,
            voice: "alloy".into(),
            speed: 1.0,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct EmbeddingConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub dimension: Option<i32>,
}

impl EmbeddingConfig {
    pub fn dimension(&self) -> i32 {
        self.dimension.unwrap_or(1024)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct ImageGenConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct McpConfig {
    pub r#type: McpTransport,
    pub command: String,
    pub args: Vec<String>,
    pub env: Option<HashMap<String, String>>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            r#type: McpTransport::Stdio,
            command: String::new(),
            args: Vec::new(),
            env: None,
        }
    }
}

/// MCP 传输协议（stdio 已实现；streamable-http 未实现）
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    Serialize,
    Deserialize,
    strum::Display,
    strum::EnumString,
    strum::IntoStaticStr,
)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum McpTransport {
    #[default]
    Stdio,
    StreamableHttp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct DatabaseConfig {
    /// PostgreSQL 连接字符串，例如：postgres://user:password@localhost/dbname
    pub url: String,
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
    pub rich_message: bool,
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
    pub r#type: Option<ProviderKind>,
    /// API key（Ollama 等本地服务可省略）
    pub api_key: Option<String>,
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

/// 被动注入配置（RAG 自动检索注入 `<knowledge>` 节）。
/// 聚合到 `[knowledge.inject]`：开关 + 条数 + 白名单，与分块参数（数据面）职责分离。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct KnowledgeInjectConfig {
    /// 被动注入开关：true = 每 turn 首轮自动检索注入 `<knowledge>` 节；
    /// false = 不注入（`search_knowledge_base` 主动工具不受影响）。
    pub enable: bool,
    pub limit: i64,
    /// collection 白名单；空 = 全部库
    pub collections: Vec<String>,
}

impl Default for KnowledgeInjectConfig {
    fn default() -> Self {
        Self {
            enable: false,
            limit: 5,
            collections: Vec::new(),
        }
    }
}

/// KnowledgeBase 配置：分块参数与 RAG 检索注入。
/// ```toml
/// [knowledge]
/// chunk-size = 512
/// chunk-overlap = 51
/// chunk-max = 1536
///
/// [knowledge.inject]
/// enable = false      # 被动注入开关；主动工具 search_knowledge_base 不受影响
/// limit = 5           # 注入块数上限
/// collections = []    # collection 白名单；空 = 全部
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct KnowledgeConfig {
    /// 目标块长（Unicode 字符）
    pub chunk_size: usize,
    /// 相邻块重叠（字符）
    pub chunk_overlap: usize,
    /// 单块硬上限（字符）
    pub chunk_max: usize,
    pub inject: KnowledgeInjectConfig,
}

impl Default for KnowledgeConfig {
    fn default() -> Self {
        Self {
            chunk_size: 512,
            chunk_overlap: 51,
            chunk_max: 1536,
            inject: KnowledgeInjectConfig::default(),
        }
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
    pub fn level(&self) -> Result<tracing::Level> {
        tracing::Level::from_str(&self.level).err_kind_msg(
            ErrorKind::Config,
            format!("Invalid logging level '{}'", self.level),
        )
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
            rich_message: raw.rich_message,
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
