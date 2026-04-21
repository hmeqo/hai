use std::{collections::HashMap, path::PathBuf, str::FromStr, sync::Arc};

use anyhow::{Result, bail};
use autoagents::llm::chat::ReasoningEffort;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use struct_patch::Patch;

use crate::agent::prompts::{GROUP_SCENE_PROMPT, PRIVATE_SCENE_PROMPT};

use crate::agent::provider::ProviderBackend;
use crate::config::PathResolver;

/// 人格配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct PersonalityConfig {
    pub name: String,
    /// 社交活跃度：0=沉默潜水，1=活跃话痨
    pub sociability: f64,
    /// 话量：0=极简，1=详尽
    pub verbosity: f64,
    /// 坦诚度：0=圆滑世故，1=坦诚直率
    pub honesty: f64,
    /// 幽默感：0=严肃正经，1=处处带梗
    pub humor: f64,
    /// 共情：0=理性冷峻，1=共情温暖
    pub empathy: f64,
    /// 情绪稳定性：0=情绪稳定，1=情绪化
    pub mood: f64,
    pub interests: Vec<String>,
    pub communication_style: String,
}

impl Default for PersonalityConfig {
    fn default() -> Self {
        Self {
            name: "hai".into(),
            sociability: 0.05,
            verbosity: 0.35,
            honesty: 0.65,
            humor: 0.70,
            empathy: 0.75,
            mood: 0.30,
            interests: vec![],
            communication_style: "口语化，偶尔用网络用语，不说教，有话直说但不冒犯".into(),
        }
    }
}

impl PersonalityConfig {
    pub fn dims(&self) -> Vec<(&str, f64, &str)> {
        vec![
            ("Sociability", self.sociability, "沉默潜水 ←→ 活跃话痨"),
            ("Verbosity", self.verbosity, "极简 ←→ 详尽"),
            ("Honesty", self.honesty, "圆滑世故 ←→ 坦诚直率"),
            ("Humor", self.humor, "严肃正经 ←→ 处处带梗"),
            ("Empathy", self.empathy, "理性冷峻 ←→ 共情温暖"),
            ("Mood", self.mood, "情绪稳定 ←→ 情绪化"),
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct AgentConfig {
    /// 当前使用的 provider 名称（对应 AppConfig.providers 中的 key）
    pub provider: String,
    pub default_model: String,
    pub system_prompt: String,
    pub group_prompt: String,
    pub private_prompt: String,
    pub personality: PersonalityConfig,
    pub debounce_ms: u64,
    pub max_tokens: u32,
    pub reasoning: bool,
    pub reasoning_effort: String,
    pub temperature: f32,
    pub format_context: bool,
    /// 草稿板总 token 上限（slot 0-9 合计）
    pub scratchpad_max_tokens: usize,
    pub trigger: TriggerConfig,
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
}

impl Default for TriggerConfig {
    fn default() -> Self {
        Self {
            min_heat: 0.02,
            min_heat_cap: 0.33,
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

/// 单个多模态子服务配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct MultimodalSubConfig {
    /// provider 名称（对应 AppConfig.providers 中的 key），为空时使用 agent.provider
    pub provider: String,
    /// 模型名称
    pub model: String,
}

impl Default for MultimodalSubConfig {
    fn default() -> Self {
        Self {
            provider: String::new(),
            model: String::new(),
        }
    }
}

/// 多模态配置
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct MultimodalConfig {
    pub image: MultimodalSubConfig,
    pub audio: MultimodalSubConfig,
    pub embedding: MultimodalSubConfig,
}

impl Default for MultimodalConfig {
    fn default() -> Self {
        Self {
            image: MultimodalSubConfig {
                provider: String::new(),
                model: "google/gemini-3.1-flash-image-preview".into(),
            },
            audio: MultimodalSubConfig {
                provider: String::new(),
                model: "openai/gpt-4o-audio-preview".into(),
            },
            embedding: MultimodalSubConfig {
                provider: String::new(),
                model: "openai/text-embedding-3-small".into(),
            },
        }
    }
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

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            provider: String::new(),
            default_model: String::new(),
            system_prompt: String::new(),
            group_prompt: GROUP_SCENE_PROMPT.into(),
            private_prompt: PRIVATE_SCENE_PROMPT.into(),
            personality: PersonalityConfig::default(),
            trigger: TriggerConfig::default(),
            debounce_ms: 1000,
            max_tokens: 8000,
            reasoning: true,
            reasoning_effort: "low".into(),
            temperature: 0.5,
            format_context: true,
            scratchpad_max_tokens: 1000,
        }
    }
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
            _ => bail!("Invalid reasoning effort: {}", self.reasoning_effort),
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(default, rename_all = "kebab-case")]
pub struct SkillsConfig {
    /// 扫描 skill 的目录列表（按顺序加载，后者同名 skill 覆盖前者）
    pub dirs: Vec<PathBuf>,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            dirs: PathResolver::skill_dirs(),
        }
    }
}
