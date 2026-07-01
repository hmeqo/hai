use genai::{Client, resolver::AuthData};
use strum::{Display, EnumIter, EnumString, IntoEnumIterator, IntoStaticStr};

use crate::{config::schema::ProviderConfig, error::Result};

/// Provider 类型枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, EnumIter, IntoStaticStr)]
#[strum(ascii_case_insensitive, serialize_all = "lowercase")]
pub enum ProviderBackend {
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

impl ProviderBackend {
    pub fn supported_types() -> Vec<&'static str> {
        Self::iter().map(Into::into).collect()
    }

    pub fn default_base_url(&self) -> &'static str {
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

    pub fn resolve_base_url(&self, override_url: Option<&str>) -> String {
        override_url
            .map(String::from)
            .unwrap_or_else(|| self.default_base_url().to_string())
    }

    fn genai_model_prefix(&self) -> &'static str {
        match self {
            Self::OpenRouter => "open_router::",
            Self::OpenAI => "",
            Self::Anthropic => "",
            Self::Google => "",
            Self::DeepSeek => "deepseek::",
            Self::Groq => "groq::",
            Self::Ollama => "",
            Self::XAI => "xai::",
            Self::AzureOpenAI => "",
            Self::MiniMax => "minimax::",
            Self::Phind => "phind::",
            Self::Requesty => "",
        }
    }
}

pub fn genai_model_name(provider: &ProviderBackend, model: &str) -> String {
    let prefix = provider.genai_model_prefix();
    format!("{prefix}{model}")
}

/// 创建 genai Client 并配置 API key。
/// 用 `AuthResolver` 而非 `set_var`，避免 unsafe。
pub fn create_genai_client(provider_config: &ProviderConfig) -> Result<Client> {
    let api_key = provider_config.api_key.clone().unwrap_or_default();
    let client = Client::builder()
        .with_auth_resolver_fn(move |_model_iden: genai::ModelIden| {
            Ok(Some(AuthData::from_single(api_key.clone())))
        })
        .build();
    Ok(client)
}
