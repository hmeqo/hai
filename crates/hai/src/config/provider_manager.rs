use std::{collections::HashMap, str::FromStr};

use genai::Client;

use crate::{
    agentcore::{
        provider::{self, Vendor},
        rawclient::{RawAgent, RawClient},
    },
    config::AppConfig,
    error::{AppResultExt, ErrorKind, OptionAppExt, Result},
};

#[derive(Clone)]
pub(crate) struct ProviderEntry {
    pub config: crate::config::schema::ProviderConfig,
    pub vendor: Vendor,
}

/// 统一管理的 provider 集合。
/// 启动时一次性解析配置文件中所有 provider（按名称存储），后续按需取用。
#[derive(Clone)]
pub struct ProviderManager {
    providers: HashMap<String, ProviderEntry>,
}

impl ProviderManager {
    pub fn new(config: &AppConfig) -> Result<Self> {
        let mut providers = HashMap::new();

        for (name, provider_cfg) in &config.providers {
            let vendor = Vendor::from_str(name).err_kind_msg(
                ErrorKind::Config,
                format!("Invalid provider type '{}'", name),
            )?;

            providers.insert(
                name.to_string(),
                ProviderEntry {
                    config: provider_cfg.clone(),
                    vendor,
                },
            );
        }

        Ok(Self { providers })
    }

    pub(crate) fn get(&self, provider: &str) -> Option<&ProviderEntry> {
        self.providers.get(provider)
    }

    pub(crate) fn get_checked(&self, provider: &str) -> Result<&ProviderEntry> {
        self.get(provider).ok_or_err_msg(
            ErrorKind::Config,
            format!("Provider '{}' not found", provider),
        )
    }

    pub fn build_client(&self, provider: &str) -> RawClient {
        let entry = self.get_checked(provider).expect("provider configured");
        let base_url = entry
            .vendor
            .resolve_base_url(entry.config.base_url.as_deref());
        RawClient::new(entry.config.api_key.as_deref(), base_url)
    }

    pub fn build_agent(&self, provider: &str, model: &str) -> RawAgent {
        self.build_client(provider).agent(model)
    }

    pub fn build_genai_client(&self, provider: &str) -> Result<Client> {
        let entry = self.get_checked(provider)?;
        provider::create_genai_client(entry)
    }
}
