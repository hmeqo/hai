use std::{collections::HashMap, str::FromStr, sync::Arc};

use crate::{
    agentcore::{RawAgent, provider::ProviderBackend, rawclient::RawClient},
    config::{AppConfig, schema::ResolvedProvider},
    error::{AppResultExt, ErrorKind, OptionAppExt, Result},
};

/// 统一管理的已解析 provider 集合
///
/// 启动时一次性解析配置文件中所有 provider（按名称存储），
/// 后续按需取用，无需再解析。
#[derive(Clone)]
pub struct ProviderManager {
    providers: HashMap<String, Arc<ResolvedProvider>>,
}

impl ProviderManager {
    pub fn new(config: &AppConfig) -> Result<Self> {
        let mut providers = HashMap::new();

        for (name, provider_cfg) in &config.providers {
            let backend = ProviderBackend::from_str(name).change_err_msg(
                ErrorKind::Config,
                format!("Invalid provider type '{}'", name),
            )?;

            let base_url = backend.resolve_base_url(provider_cfg.base_url.as_deref());

            let resolved = ResolvedProvider {
                name: name.to_string(),
                config: Arc::new(provider_cfg.clone()),
                backend,
                base_url,
            };

            providers.insert(name.to_string(), Arc::new(resolved));
        }

        Ok(Self { providers })
    }

    pub fn get(&self, provider: &str) -> Option<&Arc<ResolvedProvider>> {
        self.providers.get(provider)
    }

    pub fn get_checked(&self, provider: &str) -> Result<&Arc<ResolvedProvider>> {
        self.get(provider).ok_or_err_msg(
            ErrorKind::Config,
            format!("Provider '{}' not found", provider),
        )
    }

    pub fn build_client(&self, provider: &str) -> RawClient {
        let resolved = self.get_checked(provider).expect("provider configured");
        RawClient::new(&resolved.config.api_key, &resolved.base_url)
    }

    pub fn build_agent(&self, provider: &str, model: &str) -> RawAgent {
        self.build_client(provider).agent(model)
    }
}
