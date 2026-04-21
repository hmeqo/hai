use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use crate::agent::provider::ProviderBackend;
use crate::config::schema::ResolvedProvider;
use crate::error::{ErrorKind, Result};

/// 统一管理的已解析 provider 集合
///
/// 启动时一次性解析配置文件中所有 provider（按名称存储），
/// 后续按需取用，无需再解析。
#[derive(Clone)]
pub struct ProviderManager {
    providers: HashMap<String, Arc<ResolvedProvider>>,
}

impl ProviderManager {
    pub fn new(config: &crate::config::AppConfig) -> Result<Self> {
        let mut providers = HashMap::new();

        for (name, provider_cfg) in &config.providers {
            let backend = ProviderBackend::from_str(name).map_err(|e| {
                ErrorKind::Config.with_message(format!("Invalid provider type '{}': {}", name, e))
            })?;

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

    pub fn get(&self, name: &str) -> Option<&Arc<ResolvedProvider>> {
        self.providers.get(name)
    }

    pub fn get_checked(&self, name: &str) -> Result<&Arc<ResolvedProvider>> {
        self.get(name)
            .ok_or_else(|| ErrorKind::Config.with_message(format!("Provider '{}' not found", name)))
    }
}
