pub mod env;
pub mod manager;
pub mod meta;
pub mod paths;
pub mod provider_manager;
pub mod schema;

use std::collections::HashMap;

pub use manager::Config;
pub use paths::PathResolver;
pub use provider_manager::ProviderManager;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use struct_patch::Patch;

use crate::config::{
    manager::Configurable,
    schema::{
        AgentConfig, DatabaseConfig, GenerationModelConfig, LoggingConfig, McpConfig,
        MultimodalConfig, ProviderConfig, SkillsConfig, TelegramConfig,
    },
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch, Default)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(rename_all = "kebab-case")]
pub struct AppConfig {
    pub agent: AgentConfig,
    pub providers: HashMap<String, ProviderConfig>,
    pub multimodal: MultimodalConfig,
    pub model: HashMap<String, GenerationModelConfig>,
    pub telegram: TelegramConfig,
    pub mcp: HashMap<String, McpConfig>,
    pub skills: SkillsConfig,
    pub logging: LoggingConfig,
    pub database: DatabaseConfig,
}

impl Configurable for AppConfig {
    type Patch = AppConfigPatch;
}

pub type AppConfigManager = Config<AppConfig>;
