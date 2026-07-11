use std::sync::Arc;

use async_trait::async_trait;
use base64::{Engine as _, prelude::BASE64_STANDARD};
use serde_json::{Value, json};

use crate::{
    agentcore::{ApiClient, Endpoint, embedding::EmbeddingService},
    config::{ProviderRegistry, schema::MultimodalSubConfig},
    domain::vo::MediaCodec,
    error::{ErrorKind, Result},
};

const DEFAULT_IMAGE_PROMPT: &str = "请详细描述这张图片的全部内容，包括：画面中的主体、人物（表情、动作、着装）、物体、场景与环境、文字信息、色彩与构图、氛围与情绪，以及其他任何值得注意的细节。";
const DEFAULT_VIDEO_PROMPT: &str = "请全面分析这个视频的内容，包括：按时间顺序描述画面中发生的事件、人物（表情、动作、互动）、物体与场景、镜头切换与运镜、氛围与情绪、背景信息，以及任何值得注意的细节。";
const DEFAULT_AUDIO_PROMPT: &str = "请全面分析这段音频：按时间顺序描述不同时间段发生的事，识别说话人并逐字转写其内容（多人对话需区分说话人），分析说话人的语气、情绪和态度，识别背景音和环境信息，判断可能的场景和上下文。";
const DEFAULT_OCR_PROMPT: &str = "请提取这张图片中的所有文字内容，按原文返回。";

// ── 媒体源 ──────────────────────────────────────────────────────────────────────

pub enum MediaSource {
    Url(String),
    Bytes(Vec<u8>),
    Base64(String),
}

impl MediaSource {
    fn into_image_url(self) -> String {
        match self {
            MediaSource::Url(u) => u,
            MediaSource::Base64(b) => format!("data:image/jpeg;base64,{b}"),
            MediaSource::Bytes(d) => {
                format!("data:image/jpeg;base64,{}", BASE64_STANDARD.encode(d))
            }
        }
    }

    fn into_api_value(self, fmt: &str) -> Value {
        match self {
            MediaSource::Url(u) => json!({ "url": u }),
            MediaSource::Base64(b) => json!({ "data": b, "format": fmt }),
            MediaSource::Bytes(d) => {
                json!({ "data": BASE64_STANDARD.encode(d), "format": fmt })
            }
        }
    }
}

impl From<Vec<u8>> for MediaSource {
    fn from(bytes: Vec<u8>) -> Self {
        MediaSource::Bytes(bytes)
    }
}

// ── Modality 配置 ───────────────────────────────────────────────────────────────

#[derive(Debug)]
struct ModalityCfg {
    endpoint: Endpoint,
    params: ModalityParams,
}

#[derive(Debug)]
enum ModalityParams {
    Vision { default_prompt: String },
    Speech { voice: String, speed: f32 },
    None,
}

impl ModalityCfg {
    fn from_input(
        registry: &ProviderRegistry,
        sub: &MultimodalSubConfig,
        fallback: &str,
        default_prompt: &str,
    ) -> Result<Option<Self>> {
        if !sub.enabled() {
            return Ok(None);
        }
        let provider = sub.provider.as_deref().unwrap_or(fallback);
        let model = sub.model.as_deref().unwrap_or("");
        resolve_modality(
            registry,
            provider,
            model,
            ModalityParams::Vision {
                default_prompt: sub
                    .default_prompt
                    .clone()
                    .unwrap_or_else(|| default_prompt.into()),
            },
        )
        .map(Some)
    }
}

fn resolve_modality(
    registry: &ProviderRegistry,
    provider: &str,
    model: &str,
    params: ModalityParams,
) -> Result<ModalityCfg> {
    Ok(ModalityCfg {
        endpoint: registry.resolve(provider, model)?,
        params,
    })
}

// ── 服务 ────────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) struct MultimodalServiceInner {
    client: ApiClient,
    vision: Option<ModalityCfg>,
    video: Option<ModalityCfg>,
    audio: Option<ModalityCfg>,
    tts: Option<ModalityCfg>,
    embedding: ModalityCfg,
}

#[derive(Debug, Clone)]
pub struct MultimodalService(Arc<MultimodalServiceInner>);

#[async_trait]
impl EmbeddingService for MultimodalService {
    async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>> {
        self.0.client.embed(&self.0.embedding.endpoint, text).await
    }
}

impl MultimodalService {
    pub fn from_config(
        config: &crate::config::AppConfig,
        registry: &ProviderRegistry,
    ) -> Result<Self> {
        let fallback = &config.agent.provider;
        let mc = &config.multimodal;

        let vision =
            ModalityCfg::from_input(registry, &mc.input.image, fallback, DEFAULT_IMAGE_PROMPT)?;
        let video =
            ModalityCfg::from_input(registry, &mc.input.video, fallback, DEFAULT_VIDEO_PROMPT)?;
        let audio =
            ModalityCfg::from_input(registry, &mc.input.audio, fallback, DEFAULT_AUDIO_PROMPT)?;

        let tts = mc
            .tts
            .enabled()
            .then(|| -> Result<_> {
                let p = mc.tts.provider.as_deref().unwrap_or(fallback);
                let m = mc.tts.model.clone().unwrap_or_else(|| "tts-1".into());
                resolve_modality(
                    registry,
                    p,
                    &m,
                    ModalityParams::Speech {
                        voice: mc.tts.voice.clone(),
                        speed: mc.tts.speed,
                    },
                )
            })
            .transpose()?;

        let embedding = resolve_modality(
            registry,
            &mc.embedding.provider(fallback),
            &mc.embedding.model(),
            ModalityParams::None,
        )?;

        Ok(Self(Arc::new(MultimodalServiceInner {
            client: ApiClient::new(),
            vision,
            video,
            audio,
            tts,
            embedding,
        })))
    }

    pub async fn analyze_image(&self, source: MediaSource, prompt: Option<&str>) -> Result<String> {
        let cfg = self.0.vision.as_ref().ok_or_else(|| unavailable("图片"))?;
        let text = resolve_prompt(&cfg.params, prompt);
        let body = json!([
            {"type": "text", "text": text},
            {"type": "image_url", "image_url": {"url": source.into_image_url()}},
        ]);
        self.complete_and_extract(&cfg.endpoint, body, "vision")
            .await
    }

    pub async fn ocr(&self, source: MediaSource) -> Result<String> {
        let cfg = self.0.vision.as_ref().ok_or_else(|| unavailable("图片"))?;
        let body = json!([
            {"type": "text", "text": DEFAULT_OCR_PROMPT},
            {"type": "image_url", "image_url": {"url": source.into_image_url()}},
        ]);
        self.complete_and_extract(&cfg.endpoint, body, "vision")
            .await
    }

    pub async fn analyze_video(
        &self,
        source: MediaSource,
        format: Option<MediaCodec>,
        prompt: Option<&str>,
    ) -> Result<String> {
        let cfg = self.0.video.as_ref().ok_or_else(|| unavailable("视频"))?;
        let text = resolve_prompt(&cfg.params, prompt);
        let fmt = format.as_ref().map(|c| c.api_format()).unwrap_or("mp4");
        let body = json!([
            {"type": "text", "text": text},
            {"type": "video_url", "video_url": source.into_api_value(fmt)},
        ]);
        self.complete_and_extract(&cfg.endpoint, body, "video")
            .await
    }

    pub async fn analyze_audio(
        &self,
        source: MediaSource,
        format: Option<MediaCodec>,
        prompt: Option<&str>,
    ) -> Result<String> {
        let cfg = self.0.audio.as_ref().ok_or_else(|| unavailable("音频"))?;
        let text = resolve_prompt(&cfg.params, prompt);
        let fmt = format.as_ref().map(|c| c.api_format()).unwrap_or("wav");
        let body = json!([
            {"type": "text", "text": text},
            {"type": "input_audio", "input_audio": source.into_api_value(fmt)},
        ]);
        self.complete_and_extract(&cfg.endpoint, body, "audio")
            .await
    }

    pub async fn speech(&self, text: &str) -> Result<Vec<u8>> {
        let cfg = self.0.tts.as_ref().ok_or_else(|| unavailable("语音"))?;
        let ModalityParams::Speech { voice, speed } = &cfg.params else {
            unreachable!()
        };
        self.0
            .client
            .speech(&cfg.endpoint, text, voice, *speed)
            .await
    }

    async fn complete_and_extract(&self, ep: &Endpoint, body: Value, tag: &str) -> Result<String> {
        let resp = self.0.client.complete(ep, body).await?;
        extract_text(&resp, tag)
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────────

fn unavailable(name: &str) -> crate::error::AppError {
    ErrorKind::BadRequest.msg(format!("{name}分析不可用"))
}

fn resolve_prompt(params: &ModalityParams, custom: Option<&str>) -> String {
    let ModalityParams::Vision { default_prompt } = params else {
        unreachable!()
    };
    match custom {
        Some(c) => format!("{default_prompt}。聚焦：{c}"),
        None => default_prompt.to_string(),
    }
}

pub(crate) fn extract_text(resp: &Value, tag: &str) -> Result<String> {
    resp.pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            ErrorKind::DataParse.msg(format!("Failed to parse {tag} response: {resp:?}"))
        })
}
