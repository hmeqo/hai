use std::sync::Arc;

use async_trait::async_trait;
use base64::{Engine as _, prelude::BASE64_STANDARD};
use serde_json::{Value, json};

use crate::{
    agentcore::{ApiClient, Endpoint, embedding::EmbeddingService},
    config::ProviderRegistry,
    domain::vo::MediaCodec,
    error::{ErrorKind, Result},
};

/// 图片感官转写模板。
const IMAGE_STRUCTURE: &str = "你将看到一张图片。请把你的观察转写为文字描述——这份描述将作为唯一的视觉输入，提供给不具图像理解能力的文本模型，帮助它\"看见\"画面。按以下模板输出：\n\n\
识别: 这是什么（结合画面线索的识别结论；软件/界面、场景、物体、文档类型；识别不清写\"不确定\"+最可能类别）\n\
依据: 判断线索（标题栏文字/logo/布局特征等，一句）\n\
画面概述: 一两句整体内容与氛围\n\
布局: ASCII 字符画粗略还原画面布局（字母/符号/横线表示元素位置与大小），或文字说明方位（左/中/右、前/后、远/近）\n\
细节:\n\
- 主体: 画面各部分是什么、在什么位置\n\
- 人物: 表情/动作/姿态/互动（无则省略）\n\
- 文字: 逐字引用原文（无则省略）\n\
- 光影/构图: 光源/色调/构图要点（无则省略）\n\
- 其他: 值得注意的细节\n\n\
约束: 忠实转写画面所见；细节完整，覆盖画面主要内容与重要信息——读这份描述的人看不到原图。";

/// 视频感官转写模板。
const VIDEO_STRUCTURE: &str = "你将看到一段视频。请把你的观察转写为文字描述——这份描述将作为唯一的视觉输入，提供给不具图像理解能力的文本模型，帮助它\"看见\"画面。按以下模板输出：\n\n\
识别: 视频类型与内容定性（录屏/实拍/动画/演示等；识别不清写\"不确定\"+最可能类别）\n\
依据: 判断线索（画面特征等，一句）\n\
内容概述: 一两句主要场景与人物、整体氛围\n\
时间线: 按时间顺序分段描述发生的事件与关键动作\n\
细节:\n\
- 人物: 表情/动作/互动（无则省略）\n\
- 物体/场景: 环境特征、状态变化（无则省略）\n\
- 镜头: 切换/运镜/景别（无则省略）\n\
- 文字/对话: 逐字引用（无则省略）\n\
- 其他: 值得注意的细节\n\n\
约束: 忠实转写画面所见；时间线完整覆盖主要事件——读这份描述的人看不到原视频。";

/// 音频感官转写模板。
const AUDIO_STRUCTURE: &str = "你将听到一段音频。请把你的听觉观察转写为文字描述——这份描述将作为唯一的听觉输入，提供给不具音频理解能力的文本模型，帮助它\"听见\"内容。按以下模板输出：\n\n\
识别: 音频类型与内容定性（语音消息/会议录音/音乐/播客等；识别不清写\"不确定\"+最可能类别）\n\
依据: 判断线索（可听特征等，一句）\n\
内容概述: 一两句内容主题、可能的场景与整体氛围\n\
转写: 按时间顺序逐字转写说话内容（多人对话区分说话人）\n\
细节:\n\
- 说话人: 识别与语气/情绪/态度（无则省略）\n\
- 背景音: 环境信息（无则省略）\n\
- 其他: 值得注意的细节\n\n\
约束: 忠实转写所听内容；转写完整——读这份描述的人听不到原音频。";

/// 针对性分析提示词（focus 时覆盖默认完整转写）。
const IMAGE_JUDGMENT_PROMPT: &str =
    "你将看到一张图片。请针对下面的分析指令直接作答，基于画面所见给出聚焦于该指令的结论：";
const VIDEO_JUDGMENT_PROMPT: &str =
    "你将看到一段视频。请针对下面的分析指令直接作答，基于画面所见给出聚焦于该指令的结论：";
const AUDIO_JUDGMENT_PROMPT: &str =
    "你将听到一段音频。请针对下面的分析指令直接作答，基于所听内容给出聚焦于该指令的结论：";

/// 基础感官转写默认模板；传 focus 时由针对性分析提示词覆盖（互斥）。
const DEFAULT_IMAGE_PROMPT: &str = IMAGE_STRUCTURE;
const DEFAULT_VIDEO_PROMPT: &str = VIDEO_STRUCTURE;
const DEFAULT_AUDIO_PROMPT: &str = AUDIO_STRUCTURE;
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

/// 感官输入（vision/video/audio 共用）：endpoint + 默认转写模板
#[derive(Debug)]
struct SenseCfg {
    endpoint: Endpoint,
    default_prompt: String,
}

#[derive(Debug)]
struct TtsCfg {
    endpoint: Endpoint,
    voice: String,
    speed: f32,
}

// ── 服务 ────────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub(crate) struct MultimodalServiceInner {
    client: ApiClient,
    vision: Option<SenseCfg>,
    video: Option<SenseCfg>,
    audio: Option<SenseCfg>,
    tts: Option<TtsCfg>,
    embedding: Endpoint,
    image_gen: Option<Endpoint>,
}

#[derive(Debug, Clone)]
pub struct MultimodalService(Arc<MultimodalServiceInner>);

#[async_trait]
impl EmbeddingService for MultimodalService {
    async fn generate_embedding(&self, text: &str) -> Result<Vec<f32>> {
        self.0.client.embed(&self.0.embedding, text).await
    }
}

/// 理解类 auxiliary 块的共享访问（provider/model/enabled——块未配 = 委托主模型）
pub(crate) trait AuxInputBlock {
    fn provider(&self) -> Option<&str>;
    fn model(&self) -> Option<&str>;
    fn enabled(&self) -> bool;
}

impl AuxInputBlock for crate::config::schema::VisionConfig {
    fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }
    fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
}

impl AuxInputBlock for crate::config::schema::AudioConfig {
    fn provider(&self) -> Option<&str> {
        self.provider.as_deref()
    }
    fn model(&self) -> Option<&str> {
        self.model.as_deref()
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
}

impl MultimodalService {
    pub fn from_config(
        config: &crate::config::AppConfig,
        registry: &ProviderRegistry,
    ) -> Result<Self> {
        let fallback = &config.agent.provider;
        let main_model = &config.agent.model;
        let aux = &config.auxiliary;

        let vision = Self::resolve_input(
            registry,
            aux.vision.as_ref(),
            fallback,
            main_model,
            |b| b.image_prompt.as_deref(),
            DEFAULT_IMAGE_PROMPT,
        )?;
        let video = Self::resolve_input(
            registry,
            aux.vision.as_ref(),
            fallback,
            main_model,
            |b| b.video_prompt.as_deref(),
            DEFAULT_VIDEO_PROMPT,
        )?;
        let audio = Self::resolve_input(
            registry,
            aux.audio.as_ref(),
            fallback,
            main_model,
            |b| b.prompt.as_deref(),
            DEFAULT_AUDIO_PROMPT,
        )?;

        // 专用类：model 必配才可用（tts/embedding/image-gen）
        let tts = Self::resolve_tts(registry, aux.tts.as_ref(), fallback)?;
        let embedding = Self::resolve_embedding(registry, aux.embedding.as_ref(), fallback)?;
        let image_gen = Self::resolve_image_gen(registry, aux.image_gen.as_ref(), fallback)?;

        Ok(Self(Arc::new(MultimodalServiceInner {
            client: ApiClient::new(),
            vision,
            video,
            audio,
            tts,
            embedding,
            image_gen,
        })))
    }

    fn resolve_input<B: AuxInputBlock>(
        registry: &ProviderRegistry,
        block: Option<&B>,
        fallback_provider: &str,
        main_model: &str,
        prompt: impl Fn(&B) -> Option<&str>,
        default_prompt: &str,
    ) -> Result<Option<SenseCfg>> {
        // 块未配：委托主模型（默认可用）
        let Some(b) = block else {
            return Ok(Some(SenseCfg {
                endpoint: registry.get_endpoint(fallback_provider, main_model)?,
                default_prompt: default_prompt.into(),
            }));
        };
        if !b.enabled() {
            return Ok(None);
        }
        Ok(Some(SenseCfg {
            endpoint: registry.get_endpoint(
                b.provider().unwrap_or(fallback_provider),
                b.model().unwrap_or(main_model),
            )?,
            default_prompt: prompt(b).unwrap_or(default_prompt).into(),
        }))
    }

    fn resolve_image_gen(
        registry: &ProviderRegistry,
        block: Option<&crate::config::schema::ImageGenConfig>,
        fallback_provider: &str,
    ) -> Result<Option<Endpoint>> {
        // 专用类：model 必配才可用
        let Some(b) = block else { return Ok(None) };
        let model = b.model.as_deref().unwrap_or("");
        if model.is_empty() {
            return Ok(None);
        }
        registry
            .get_endpoint(b.provider.as_deref().unwrap_or(fallback_provider), model)
            .map(Some)
    }

    fn resolve_tts(
        registry: &ProviderRegistry,
        block: Option<&crate::config::schema::TtsConfig>,
        fallback_provider: &str,
    ) -> Result<Option<TtsCfg>> {
        // 专用类：model 必配才可用
        let Some(b) = block else { return Ok(None) };
        let model = b.model.as_deref().unwrap_or("");
        if model.is_empty() {
            return Ok(None);
        }
        Ok(Some(TtsCfg {
            endpoint: registry
                .get_endpoint(b.provider.as_deref().unwrap_or(fallback_provider), model)?,
            voice: b.voice.clone(),
            speed: b.speed,
        }))
    }

    fn resolve_embedding(
        registry: &ProviderRegistry,
        block: Option<&crate::config::schema::EmbeddingConfig>,
        fallback_provider: &str,
    ) -> Result<Endpoint> {
        // 契约式失败：embedding 是必需能力——未配或 model 空 = 违约，快速失败（不静默回退）
        let Some(b) = block else {
            return Err(ErrorKind::Config.msg(
                "[auxiliary.embedding] is required (memory/retrieval depends on it) but not configured",
            ));
        };
        let model = b.model.as_deref().unwrap_or("");
        if model.is_empty() {
            return Err(ErrorKind::Config.msg(
                "[auxiliary.embedding] is required (memory/retrieval depends on it) but model is empty",
            ));
        }
        registry.get_endpoint(b.provider.as_deref().unwrap_or(fallback_provider), model)
    }

    pub async fn analyze_image(&self, source: MediaSource, focus: Option<&str>) -> Result<String> {
        let cfg = self.0.vision.as_ref().ok_or_else(|| unavailable("图片"))?;
        let text = build_prompt(IMAGE_JUDGMENT_PROMPT, &cfg.default_prompt, focus);
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
        focus: Option<&str>,
    ) -> Result<String> {
        let cfg = self.0.video.as_ref().ok_or_else(|| unavailable("视频"))?;
        let text = build_prompt(VIDEO_JUDGMENT_PROMPT, &cfg.default_prompt, focus);
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
        focus: Option<&str>,
    ) -> Result<String> {
        let cfg = self.0.audio.as_ref().ok_or_else(|| unavailable("音频"))?;
        let text = build_prompt(AUDIO_JUDGMENT_PROMPT, &cfg.default_prompt, focus);
        let fmt = format.as_ref().map(|c| c.api_format()).unwrap_or("wav");
        let body = json!([
            {"type": "text", "text": text},
            {"type": "input_audio", "input_audio": source.into_api_value(fmt)},
        ]);
        self.complete_and_extract(&cfg.endpoint, body, "audio")
            .await
    }

    pub fn image_gen_enabled(&self) -> bool {
        self.0.image_gen.is_some()
    }

    pub fn tts_enabled(&self) -> bool {
        self.0.tts.is_some()
    }

    pub async fn generate_image(
        &self,
        prompt: &str,
        images: &[crate::agentcore::apiclient::ImageUrl],
    ) -> Result<Vec<u8>> {
        let ep = self
            .0
            .image_gen
            .as_ref()
            .ok_or_else(|| unavailable("图像生成"))?;
        self.0.client.generate_image(ep, prompt, images).await
    }

    pub async fn speech(&self, text: &str) -> Result<Vec<u8>> {
        let cfg = self.0.tts.as_ref().ok_or_else(|| unavailable("语音"))?;
        self.0
            .client
            .speech(&cfg.endpoint, text, &cfg.voice, cfg.speed)
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

fn build_prompt(judgment: &str, default_prompt: &str, focus: Option<&str>) -> String {
    match focus {
        Some(f) => format!("{judgment}\n{f}"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    /// 块级缺省：`[auxiliary.vision]` 只配 model → enabled 默认 true、model 生效
    #[test]
    fn vision_enabled_defaults_true() {
        let cfg: crate::config::schema::VisionConfig =
            toml::from_str("model = \"gpt-4o\"").unwrap();
        assert!(cfg.enabled);
        assert_eq!(cfg.model.as_deref(), Some("gpt-4o"));
    }

    /// auxiliary.image-gen 配置后 from_config 能解析出 image_gen 角色
    #[test]
    fn image_gen_from_config() {
        let mut cfg = AppConfig::default();
        cfg.agent.provider = "openai".into();
        cfg.agent.model = "gpt-4o".into();
        cfg.providers.insert("openai".into(), Default::default());
        // embedding 是必需能力（契约式失败）——测试须配置
        cfg.auxiliary.embedding = Some(crate::config::schema::EmbeddingConfig {
            provider: Some("openai".into()),
            model: Some("text-embedding-3-small".into()),
            dimension: None,
        });
        cfg.auxiliary.image_gen = Some(crate::config::schema::ImageGenConfig {
            provider: Some("openai".into()),
            model: Some("dall-e-3".into()),
        });
        let registry = crate::config::provider_manager::ProviderRegistry::new(&cfg).unwrap();
        let svc = MultimodalService::from_config(&cfg, &registry).unwrap();
        assert!(svc.image_gen_enabled());
    }

    /// 条件挂载规则：auxiliary.image-gen 未配 → disabled
    #[test]
    fn image_gen_disabled_when_unconfigured() {
        let mut cfg = AppConfig::default();
        cfg.agent.provider = "openai".into();
        cfg.agent.model = "gpt-4o".into();
        cfg.providers.insert("openai".into(), Default::default());
        cfg.auxiliary.embedding = Some(crate::config::schema::EmbeddingConfig {
            provider: Some("openai".into()),
            model: Some("text-embedding-3-small".into()),
            dimension: None,
        });
        // 不配 image_gen
        let registry = crate::config::provider_manager::ProviderRegistry::new(&cfg).unwrap();
        let svc = MultimodalService::from_config(&cfg, &registry).unwrap();
        assert!(!svc.image_gen_enabled());
    }

    /// 条件挂载规则：auxiliary.image-gen 配了但 model 空 → disabled
    #[test]
    fn image_gen_disabled_when_model_empty() {
        let mut cfg = AppConfig::default();
        cfg.agent.provider = "openai".into();
        cfg.agent.model = "gpt-4o".into();
        cfg.providers.insert("openai".into(), Default::default());
        cfg.auxiliary.embedding = Some(crate::config::schema::EmbeddingConfig {
            provider: Some("openai".into()),
            model: Some("text-embedding-3-small".into()),
            dimension: None,
        });
        cfg.auxiliary.image_gen = Some(crate::config::schema::ImageGenConfig {
            provider: Some("openai".into()),
            model: None,
        });
        let registry = crate::config::provider_manager::ProviderRegistry::new(&cfg).unwrap();
        let svc = MultimodalService::from_config(&cfg, &registry).unwrap();
        assert!(!svc.image_gen_enabled());
    }

    /// provider 缺省回退 agent.provider（image_gen 只配 model）
    #[test]
    fn image_gen_provider_falls_back_to_agent() {
        let mut cfg = AppConfig::default();
        cfg.agent.provider = "openai".into();
        cfg.agent.model = "gpt-4o".into();
        cfg.providers.insert("openai".into(), Default::default());
        cfg.auxiliary.embedding = Some(crate::config::schema::EmbeddingConfig {
            provider: Some("openai".into()),
            model: Some("text-embedding-3-small".into()),
            dimension: None,
        });
        cfg.auxiliary.image_gen = Some(crate::config::schema::ImageGenConfig {
            provider: None, // 缺省 → agent.provider
            model: Some("dall-e-3".into()),
        });
        let registry = crate::config::provider_manager::ProviderRegistry::new(&cfg).unwrap();
        let svc = MultimodalService::from_config(&cfg, &registry).unwrap();
        assert!(svc.image_gen_enabled());
    }
}
