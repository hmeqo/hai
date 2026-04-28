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
    error::{OptionAppExt, Result},
};

const DEFAULT_IMAGE_PROMPT: &str = "请详细描述这张图片的全部内容，包括：画面中的主体、人物（表情、动作、着装）、物体、场景与环境、文字信息、色彩与构图、氛围与情绪，以及其他任何值得注意的细节。";
const DEFAULT_VIDEO_PROMPT: &str = "请全面分析这个视频的内容，包括：按时间顺序描述画面中发生的事件、人物（表情、动作、互动）、物体与场景、镜头切换与运镜、氛围与情绪、背景信息，以及任何值得注意的细节。";
const DEFAULT_AUDIO_PROMPT: &str = "请全面分析这段音频：按时间顺序描述不同时间段发生的事，识别说话人并逐字转写其内容（多人对话需区分说话人），分析说话人的语气、情绪和态度，识别背景音和环境信息，判断可能的场景和上下文。";
const DEFAULT_OCR_PROMPT: &str = "请提取这张图片中的所有文字内容，按原文返回。";

pub enum MediaInput {
    Data(Vec<u8>),
    Base64(String),
    Url(String),
}

impl From<Vec<u8>> for MediaInput {
    fn from(data: Vec<u8>) -> Self {
        MediaInput::Data(data)
    }
}

impl MediaInput {
    fn into_url(self) -> String {
        match self {
            MediaInput::Url(u) => u,
            MediaInput::Base64(b) => format!("data:image/jpeg;base64,{}", b),
            MediaInput::Data(d) => {
                format!("data:image/jpeg;base64,{}", BASE64_STANDARD.encode(d))
            }
        }
    }

    fn into_audio(self, format: &str) -> Value {
        match self {
            MediaInput::Url(u) => json!({"url": u}),
            MediaInput::Base64(b) => json!({"data": b, "format": format}),
            MediaInput::Data(d) => {
                json!({"data": BASE64_STANDARD.encode(d), "format": format})
            }
        }
    }
}

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
        let build_agent = |provider: Option<&str>, model: Option<&str>| -> _ {
            providers.build_agent(
                provider.unwrap_or(&config.agent.provider),
                model.unwrap_or(""),
            )
        };

        let tts_cfg = &config.multimodal.tts;
        let tts_speech = if tts_cfg.enabled() {
            let provider = tts_cfg
                .provider
                .as_deref()
                .unwrap_or(&config.agent.provider);
            let model = tts_cfg.model.clone().unwrap_or_else(|| "tts-1".into());
            providers.build_agent(provider, &model)
        } else {
            RawAgent::new(RawClient::new("", ""), "")
        };
        let tts_service = TtsService::new(tts_speech, tts_cfg);

        Self(Arc::new(MultimodalServiceInner {
            image: build_agent(
                mc.input.image.provider.as_deref(),
                mc.input.image.model.as_deref(),
            )
            .with_default_prompt(
                mc.input
                    .image
                    .default_prompt
                    .clone()
                    .unwrap_or_else(|| DEFAULT_IMAGE_PROMPT.into()),
            ),
            video: build_agent(
                mc.input.video.provider.as_deref(),
                mc.input.video.model.as_deref(),
            )
            .with_default_prompt(
                mc.input
                    .video
                    .default_prompt
                    .clone()
                    .unwrap_or_else(|| DEFAULT_VIDEO_PROMPT.into()),
            ),
            audio: build_agent(
                mc.input.audio.provider.as_deref(),
                mc.input.audio.model.as_deref(),
            )
            .with_default_prompt(
                mc.input
                    .audio
                    .default_prompt
                    .clone()
                    .unwrap_or_else(|| DEFAULT_AUDIO_PROMPT.into()),
            ),
            embedding: build_agent(
                mc.embedding.provider.as_deref(),
                mc.embedding.model.as_deref(),
            ),
            tts: tts_service,
        }))
    }

    pub async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>> {
        self.0.embedding.embedding(text).await
    }

    async fn completion(&self, agent: &RawAgent, content: Value, tag: &str) -> Result<String> {
        let resp = agent.completion(content, None::<Value>).await?;
        extract_text(&resp, tag)
    }

    pub async fn analyze_image(
        &self,
        image: impl Into<MediaInput>,
        prompt: Option<&str>,
    ) -> Result<String> {
        let prompt = prompt.unwrap_or(&self.0.image.default_prompt);
        let url = image.into().into_url();
        let content = json!([
            {"type": "text", "text": prompt},
            {
                "type": "image_url",
                "image_url": {"url": url, "detail": "high"}
            },
        ]);
        self.completion(&self.0.image, content, "vision").await
    }

    pub async fn ocr(&self, image: impl Into<MediaInput>) -> Result<String> {
        let url = image.into().into_url();
        let content = json!([
            {"type": "text", "text": DEFAULT_OCR_PROMPT},
            {
                "type": "image_url",
                "image_url": {"url": url, "detail": "high"}
            },
        ]);
        self.completion(&self.0.image, content, "vision").await
    }

    pub async fn analyze_video(
        &self,
        video: impl Into<MediaInput>,
        prompt: Option<&str>,
    ) -> Result<String> {
        let prompt = prompt.unwrap_or(&self.0.video.default_prompt);
        let url = video.into().into_url();
        let content = json!([
            {"type": "text", "text": prompt},
            {"type": "video_url", "video_url": {"url": url}},
        ]);
        self.completion(&self.0.video, content, "video").await
    }

    pub async fn analyze_audio(
        &self,
        audio: impl Into<MediaInput>,
        prompt: Option<&str>,
        format: Option<&str>,
    ) -> Result<String> {
        let prompt = prompt.unwrap_or(&self.0.audio.default_prompt);
        let format = format.unwrap_or("wav");
        let audio = audio.into().into_audio(format);
        let content = json!([
            {"type": "text", "text": prompt},
            {
                "type": "input_audio",
                "input_audio": audio
            },
        ]);
        self.completion(&self.0.audio, content, "audio").await
    }

    pub async fn speech(&self, prompt: &str) -> Result<Vec<u8>> {
        self.0.tts.speech(prompt).await
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
            format!("Failed to parse {} response: {:?}", tag, resp),
        )
        .map(|s| s.to_string())
}
