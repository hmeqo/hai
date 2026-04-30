pub mod model;
pub mod tts;

use std::sync::Arc;

use base64::{Engine as _, prelude::BASE64_STANDARD};
use derive_more::Deref;
pub use model::*;
use serde_json::{Value, json};
pub use tts::*;

use crate::{
    agentcore::rawclient::{RawAgent, RawClient},
    config::{AppConfig, ProviderManager},
    domain::vo::MediaCodec,
    error::{OptionAppExt, Result},
};

const DEFAULT_IMAGE_PROMPT: &str = "请详细描述这张图片的全部内容，包括：画面中的主体、人物（表情、动作、着装）、物体、场景与环境、文字信息、色彩与构图、氛围与情绪，以及其他任何值得注意的细节。";
const DEFAULT_VIDEO_PROMPT: &str = "请全面分析这个视频的内容，包括：按时间顺序描述画面中发生的事件、人物（表情、动作、互动）、物体与场景、镜头切换与运镜、氛围与情绪、背景信息，以及任何值得注意的细节。";
const DEFAULT_AUDIO_PROMPT: &str = "请全面分析这段音频：按时间顺序描述不同时间段发生的事，识别说话人并逐字转写其内容（多人对话需区分说话人），分析说话人的语气、情绪和态度，识别背景音和环境信息，判断可能的场景和上下文。";
const DEFAULT_OCR_PROMPT: &str = "请提取这张图片中的所有文字内容，按原文返回。";

// ── 媒体输入 ──────────────────────────────────────────────────────────────────

/// 多模态媒体输入：统一表示图片、音频、视频的数据来源和可选格式。
///
/// - 图片场景不需要 `format`，直接构造 `Url` / `Bytes` 即可。
/// - 音频 / 视频场景通过 `format` 携带编码格式；`None` 时由各 `analyze_*` 方法回退到默认值。
pub enum MediaSource {
    Url(String),
    Bytes(Vec<u8>),
    /// 已经完成 base64 编码的数据（避免重复编码）。
    Base64(String),
}

/// 携带数据来源和可选格式的媒体输入，用于音频 / 视频分析。
pub struct MediaInput {
    pub source: MediaSource,
    pub format: Option<MediaCodec>,
}

impl MediaInput {
    pub fn from_bytes(bytes: Vec<u8>, format: Option<MediaCodec>) -> Self {
        Self {
            source: MediaSource::Bytes(bytes),
            format,
        }
    }

    pub fn from_url(url: String, format: Option<MediaCodec>) -> Self {
        Self {
            source: MediaSource::Url(url),
            format,
        }
    }

    /// 序列化为 `input_audio` / `video_url` 兼容的 JSON 对象。
    fn into_api_value(self, format: &str) -> Value {
        match self.source {
            MediaSource::Url(u) => json!({ "url": u }),
            MediaSource::Base64(b) => json!({ "data": b, "format": format }),
            MediaSource::Bytes(d) => json!({ "data": BASE64_STANDARD.encode(d), "format": format }),
        }
    }

    /// 序列化为图片 `image_url` 兼容的 data URL 或原始 URL。
    fn into_image_url(self) -> String {
        match self.source {
            MediaSource::Url(u) => u,
            MediaSource::Base64(b) => format!("data:image/jpeg;base64,{b}"),
            MediaSource::Bytes(d) => {
                format!("data:image/jpeg;base64,{}", BASE64_STANDARD.encode(d))
            }
        }
    }
}

impl From<Vec<u8>> for MediaInput {
    fn from(bytes: Vec<u8>) -> Self {
        Self::from_bytes(bytes, None)
    }
}

// ── MultimodalService ─────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct MultimodalServiceInner {
    image: RawAgent,
    video: RawAgent,
    audio: RawAgent,
    embedding: RawAgent,
    pub(crate) tts: TtsService,
}

#[derive(Debug, Clone, Deref)]
pub struct MultimodalService(Arc<MultimodalServiceInner>);

impl MultimodalService {
    pub fn from_config(config: &AppConfig, providers: &ProviderManager) -> Self {
        let mc = &config.multimodal;

        let build_agent = |provider: Option<&str>, model: Option<&str>| {
            providers.build_agent(
                provider.unwrap_or(&config.agent.provider),
                model.unwrap_or(""),
            )
        };
        let build_input_agent = |sub: &crate::config::schema::MultimodalSubConfig,
                                 default_prompt: &str| {
            build_agent(sub.provider.as_deref(), sub.model.as_deref()).with_default_prompt(
                sub.default_prompt
                    .clone()
                    .unwrap_or_else(|| default_prompt.into()),
            )
        };

        let tts_cfg = &mc.tts;
        let tts_agent = if tts_cfg.enabled() {
            let provider = tts_cfg
                .provider
                .as_deref()
                .unwrap_or(&config.agent.provider);
            let model = tts_cfg.model.clone().unwrap_or_else(|| "tts-1".into());
            providers.build_agent(provider, &model)
        } else {
            RawAgent::new(RawClient::new("", ""), "")
        };

        Self(Arc::new(MultimodalServiceInner {
            image: build_input_agent(&mc.input.image, DEFAULT_IMAGE_PROMPT),
            video: build_input_agent(&mc.input.video, DEFAULT_VIDEO_PROMPT),
            audio: build_input_agent(&mc.input.audio, DEFAULT_AUDIO_PROMPT),
            embedding: build_agent(
                mc.embedding.provider.as_deref(),
                mc.embedding.model.as_deref(),
            ),
            tts: TtsService::new(tts_agent, tts_cfg),
        }))
    }

    pub async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>> {
        self.0.embedding.embedding(text).await
    }

    pub async fn analyze_image(
        &self,
        image: impl Into<MediaInput>,
        prompt: Option<&str>,
    ) -> Result<String> {
        let prompt = prompt.unwrap_or(&self.0.image.default_prompt);
        let url = image.into().into_image_url();
        let content = json!([
            {"type": "text", "text": prompt},
            {"type": "image_url", "image_url": {"url": url, "detail": "high"}},
        ]);
        self.completion(&self.0.image, content, "vision").await
    }

    pub async fn ocr(&self, image: impl Into<MediaInput>) -> Result<String> {
        let url = image.into().into_image_url();
        let content = json!([
            {"type": "text", "text": DEFAULT_OCR_PROMPT},
            {"type": "image_url", "image_url": {"url": url, "detail": "high"}},
        ]);
        self.completion(&self.0.image, content, "vision").await
    }

    pub async fn analyze_video(
        &self,
        video: impl Into<MediaInput>,
        prompt: Option<&str>,
    ) -> Result<String> {
        let prompt = prompt.unwrap_or(&self.0.video.default_prompt);
        let video = video.into();
        let format = video.format.map(|c| c.api_format()).unwrap_or("mp4");
        let content = json!([
            {"type": "text", "text": prompt},
            {"type": "video_url", "video_url": video.into_api_value(format)},
        ]);
        self.completion(&self.0.video, content, "video").await
    }

    pub async fn analyze_audio(
        &self,
        audio: impl Into<MediaInput>,
        prompt: Option<&str>,
    ) -> Result<String> {
        let prompt = prompt.unwrap_or(&self.0.audio.default_prompt);
        let audio = audio.into();
        let format = audio.format.map(|c| c.api_format()).unwrap_or("wav");
        tracing::debug!(format, "analyze_audio");
        let content = json!([
            {"type": "text", "text": prompt},
            {"type": "input_audio", "input_audio": audio.into_api_value(format)},
        ]);
        self.completion(&self.0.audio, content, "audio").await
    }

    pub async fn speech(&self, prompt: &str) -> Result<Vec<u8>> {
        self.0.tts.speech(prompt).await
    }

    async fn completion(&self, agent: &RawAgent, content: Value, tag: &str) -> Result<String> {
        let resp = agent.completion(content, None::<Value>).await?;
        extract_text(&resp, tag)
    }
}

pub(crate) fn extract_text(resp: &Value, tag: &str) -> Result<String> {
    use crate::error::ErrorKind;
    resp.get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_err_msg(
            ErrorKind::DataParse,
            format!("Failed to parse {tag} response: {resp:?}"),
        )
        .map(|s| s.to_string())
}
