pub mod manager;
pub mod schema;

pub use manager::Config;

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::HashMap;
use struct_patch::Patch;

use crate::config::manager::Configurable;
use crate::config::schema::{
    AgentConfig, DatabaseConfig, LoggingConfig, McpConfig, TelegramConfig,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Patch)]
#[patch(attribute(derive(Debug, Default, Clone, Serialize, Deserialize)))]
#[patch(attribute(skip_serializing_none))]
#[serde(rename_all = "kebab-case")]
pub struct AppConfig {
    pub agent: AgentConfig,
    pub telegram: TelegramConfig,
    pub mcp: HashMap<String, McpConfig>,
    pub logging: LoggingConfig,
    pub database: DatabaseConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            agent: AgentConfig::default(),
            telegram: TelegramConfig::default(),
            mcp: HashMap::new(),
            logging: LoggingConfig::default(),
            database: DatabaseConfig::default(),
        }
    }
}

impl Configurable for AppConfig {
    type Patch = AppConfigPatch;
}

pub type AppConfigManager = Config<AppConfig>;
