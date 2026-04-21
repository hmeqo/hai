use std::sync::Arc;

use anyhow::{Result, bail};
use autoagents::llm::LLMProvider;
use autoagents::llm::chat::ReasoningEffort;
use autoagents::prelude::LLMBuilder;
use strum::{Display, EnumIter, EnumString, IntoEnumIterator, IntoStaticStr};

/// 支持的 LLM 提供商（含 autoagents 原生支持 + 项目自定义）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString, EnumIter, IntoStaticStr)]
#[strum(ascii_case_insensitive, serialize_all = "kebab-case")]
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

/// LLM provider 构建参数
pub struct LlmBuildConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub reasoning: bool,
    pub reasoning_effort: ReasoningEffort,
    pub temperature: f32,
    pub max_tokens: u32,
}

/// 解析后的 provider 信息（包含 backend 和 base_url）
pub struct ResolvedProviderInfo {
    pub backend: ProviderBackend,
    pub base_url: String,
    pub api_key: String,
}

impl ProviderBackend {
    /// 所有支持的 provider 类型名称（逗号分隔）
    pub fn supported_types() -> Vec<&'static str> {
        Self::iter().map(Into::into).collect()
    }

    /// 该 provider 的默认 base_url
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
            Self::MiniMax => "https://api.minimaxi.chat/v1",
            Self::Phind => "https://api.phind.com",
            Self::Requesty => "https://router.requesty.ai/v1",
        }
    }

    /// 解析 base_url（优先使用覆盖值，否则使用默认值）
    pub fn resolve_base_url(&self, override_url: Option<&str>) -> String {
        override_url
            .map(String::from)
            .unwrap_or_else(|| self.default_base_url().to_string())
    }

    /// 根据配置构建 LLM provider
    pub fn build(self, cfg: LlmBuildConfig) -> Result<Arc<dyn LLMProvider>> {
        macro_rules! build_provider {
            ($ty:ty) => {{
                let builder: LLMBuilder<$ty> = LLMBuilder::new()
                    .api_key(&cfg.api_key)
                    .base_url(&cfg.base_url)
                    .model(&cfg.model)
                    .reasoning(cfg.reasoning)
                    .reasoning_effort(cfg.reasoning_effort)
                    .temperature(cfg.temperature)
                    .max_tokens(cfg.max_tokens);
                builder
                    .build()
                    .map(|arc| arc as Arc<dyn LLMProvider>)
                    .map_err(Into::into)
            }};
        }

        match self {
            Self::OpenRouter => build_provider!(autoagents::llm::backends::openrouter::OpenRouter),
            Self::OpenAI | Self::Requesty => {
                build_provider!(autoagents::llm::backends::openai::OpenAI)
            }
            Self::Anthropic => build_provider!(autoagents::llm::backends::anthropic::Anthropic),
            Self::Google => build_provider!(autoagents::llm::backends::google::Google),
            Self::DeepSeek => build_provider!(autoagents::llm::backends::deepseek::DeepSeek),
            Self::Groq => build_provider!(autoagents::llm::backends::groq::Groq),
            Self::Ollama => build_provider!(autoagents::llm::backends::ollama::Ollama),
            Self::XAI => build_provider!(autoagents::llm::backends::xai::XAI),
            other => bail!(
                "LLM provider '{other}' is not yet supported. \
                 Supported: openrouter, openai, anthropic, google, deepseek, groq, ollama, xai, requesty",
            ),
        }
    }
}
