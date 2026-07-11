use std::{collections::HashMap, str::FromStr};

use crate::{
    agentcore::{Endpoint, provider::Vendor},
    config::AppConfig,
    error::{AppResultExt, ErrorKind, OptionAppExt, Result},
};

#[derive(Debug, Clone)]
pub(crate) struct ProviderEntry {
    pub config: crate::config::schema::ProviderConfig,
    pub vendor: Vendor,
}

#[derive(Debug, Clone)]
pub struct ProviderRegistry {
    providers: HashMap<String, ProviderEntry>,
}

impl ProviderRegistry {
    pub fn new(config: &AppConfig) -> Result<Self> {
        let mut providers = HashMap::new();

        for (name, provider_cfg) in &config.providers {
            let vendor_name = provider_cfg.r#type.as_deref().unwrap_or(name);
            let vendor = Vendor::from_str(vendor_name).err_kind_msg(
                ErrorKind::Config,
                format!("Invalid provider type '{vendor_name}'"),
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
            format!("Provider '{provider}' not found"),
        )
    }

    /// 解析 provider 的连接参数 + 模型名，返回 `Endpoint`。
    pub fn resolve(&self, provider: &str, model: &str) -> Result<Endpoint> {
        let entry = self.get_checked(provider)?;
        let base_url = entry
            .vendor
            .resolve_base_url(entry.config.base_url.as_deref());
        let api_key = entry.config.api_key.clone().unwrap_or_default();
        Ok(Endpoint {
            base_url,
            api_key,
            model: model.to_owned(),
        })
    }
}
