use genai::{
    Client, ServiceTarget,
    adapter::AdapterKind,
    resolver::{AuthData, Endpoint},
};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString, IntoStaticStr};

use crate::{
    config::provider_manager::ProviderEntry, error::Result, util::url::ensure_trailing_slash,
};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, EnumString, IntoStaticStr,
)]
#[serde(rename_all = "lowercase")]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum ProviderKind {
    OpenRouter,
    OpenAI,
    Anthropic,
    Google,
    DeepSeek,
    Groq,
    Ollama,
    XAI,
    AzureOpenAI,
    MiniMax,
    Phind,
    Requesty,
}

impl ProviderKind {
    pub(crate) fn default_base_url(&self) -> &'static str {
        match self {
            Self::OpenRouter => "https://openrouter.ai/api/v1",
            Self::OpenAI => "https://api.openai.com/v1",
            Self::Anthropic => "https://api.anthropic.com",
            Self::Google => "https://generativelanguage.googleapis.com",
            Self::DeepSeek => "https://api.deepseek.com/v1",
            Self::Groq => "https://api.groq.com/openai/v1",
            Self::Ollama => "http://localhost:11434/v1",
            Self::XAI => "https://api.x.ai/v1",
            Self::AzureOpenAI => "",
            Self::MiniMax => "https://api.minimax.io/v1",
            Self::Phind => "https://api.phind.com",
            Self::Requesty => "https://router.requesty.ai/v1",
        }
    }

    pub(crate) fn resolve_base_url(&self, override_url: Option<&str>) -> String {
        override_url
            .map(String::from)
            .unwrap_or_else(|| self.default_base_url().to_string())
    }

    fn genai_adapter_kind(&self) -> AdapterKind {
        match self {
            Self::OpenRouter => AdapterKind::OpenRouter,
            Self::OpenAI => AdapterKind::OpenAI,
            Self::Anthropic => AdapterKind::Anthropic,
            Self::Google => AdapterKind::Gemini,
            Self::DeepSeek => AdapterKind::DeepSeek,
            Self::Groq => AdapterKind::Groq,
            Self::Ollama => AdapterKind::Ollama,
            Self::XAI => AdapterKind::Xai,
            Self::AzureOpenAI => AdapterKind::OpenAI,
            Self::MiniMax => AdapterKind::MiniMax,
            Self::Phind => AdapterKind::OpenAI,
            Self::Requesty => AdapterKind::OpenAI,
        }
    }
}

pub(crate) fn create_genai_client(entry: &ProviderEntry) -> Result<Client> {
    let api_key = entry.config.api_key.clone().unwrap_or_default();
    let base_url = ensure_trailing_slash(
        entry
            .kind
            .resolve_base_url(entry.config.base_url.as_deref()),
    );

    Ok(Client::builder()
        .with_adapter_kind(entry.kind.genai_adapter_kind())
        .with_auth_resolver_fn(move |_| Ok(Some(AuthData::from_single(api_key))))
        .with_service_target_resolver_fn(move |mut target: ServiceTarget| {
            target.endpoint = Endpoint::from_owned(base_url);
            Ok(target)
        })
        .build())
}
