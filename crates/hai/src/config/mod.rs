pub mod manager;
pub mod paths;
pub mod provider_manager;
pub mod schema;

pub use manager::Config;
pub use paths::PathResolver;
pub use provider_manager::ProviderManager;

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::HashMap;
use struct_patch::Patch;

use crate::config::manager::Configurable;
use crate::config::schema::{
    AgentConfig, DatabaseConfig, LoggingConfig, McpConfig, MultimodalConfig, ProviderConfig,
    SkillsConfig, TelegramConfig,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch, Default)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(rename_all = "kebab-case")]
pub struct AppConfig {
    pub agent: AgentConfig,
    pub providers: HashMap<String, ProviderConfig>,
    pub multimodal: MultimodalConfig,
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
