pub mod conversation;
mod load;
pub mod schema;

use std::collections::HashMap;

pub use load::{ConfigIssue, Paths, env};
use serde::{Deserialize, Serialize};

use crate::config::schema::{
    AgentConfig, ChannelSection, ConversationConfig, DatabaseConfig, KnowledgeConfig,
    LoggingConfig, ModelRolesConfig, PromptConfig, ProviderConfig, SandboxConfig, SkillsConfig,
    ToolsConfig, WebConfig,
};

pub(crate) const PROJECT_NAME: &str = "hai";
pub(crate) const PROJECT_NAME_UPPER: &str = "HAI";
pub(crate) const AGENT_NAME: &str = "hai";

/// The whole config; [`AppConfig::load`] resolves it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct AppConfig {
    pub agent: AgentConfig,
    pub conversation: ConversationConfig,
    pub models: std::collections::BTreeMap<String, String>,
    pub prompt: PromptConfig,
    pub knowledge: KnowledgeConfig,
    pub providers: HashMap<String, ProviderConfig>,
    pub model_roles: ModelRolesConfig,
    pub web: WebConfig,
    pub channel: HashMap<String, ChannelSection>,
    pub mcp: HashMap<String, crate::agentcore::mcp::McpConfig>,
    pub sandbox: SandboxConfig,
    pub skills: SkillsConfig,
    pub tools: ToolsConfig,
    pub logging: LoggingConfig,
    pub database: DatabaseConfig,
}
