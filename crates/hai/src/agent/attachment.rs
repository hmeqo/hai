use std::sync::Arc;

use base64::{Engine as _, prelude::BASE64_STANDARD};
use uuid::Uuid;

use crate::{
    agentcore::multimodal::{MediaInput, MultimodalService},
    bot::telegram::TelegramService,
    domain::{
        service::DbServices,
        vo::{AttachmentParser, MediaCodec, Source, TelegramContentPart},
    },
    error::{ErrorKind, OptionAppExt, Result},
    infra::cache::FileCache,
};

#[derive(Debug, Clone)]
pub struct AttachmentService(Arc<AttachmentServiceInner>);

#[derive(Debug)]
struct AttachmentServiceInner {
    cache: FileCache,
    telegram_file: TelegramService,
    db_srv: DbServices,
    multimodal: MultimodalService,
}

impl AttachmentService {
    pub fn new(
        cache: FileCache,
        telegram_file: TelegramService,
        db_srv: DbServices,
        multimodal: MultimodalService,
    ) -> Self {
        Self(Arc::new(AttachmentServiceInner {
            cache,
            telegram_file,
            db_srv,
            multimodal,
        }))
    }

    /// 下载 Telegram 文件，带磁盘缓存（sticker 等需要下载的场景使用）
    async fn download_telegram(&self, file_id: &str) -> Result<Vec<u8>> {
        let cache_key = format!("tg:{}", file_id);
        if let Some(cached) = self.0.cache.find(&cache_key) {
            return Ok(cached);
        }
        let data = self.0.telegram_file.download(file_id).await?;
        self.0.cache.add(&cache_key, &data)?;
        Ok(data)
    }

    /// 通用模式：查 perception 缓存 → 执行分析 → 写缓存
    async fn with_perception_cache(
        &self,
        source: Source,
        parser: AttachmentParser,
        prompt: Option<&str>,
        analyze: impl std::future::Future<Output = Result<String>>,
    ) -> Result<String> {
        if let Some(cached) = self
            .0
            .db_srv
            .perception
            .find(&source, parser.name(), prompt)
            .await?
        {
            return Ok(cached.content);
        }

        let content = analyze.await?;

        self.0
            .db_srv
            .perception
            .upsert(&source, parser.name(), prompt, &content)
            .await?;

        Ok(content)
    }

    /// 分析消息附件（通过 attachment_id 关联）。
    /// 1. 从消息中找到 file_id
    /// 2. source = Source::Platform { platform, file_id }，perception 以此缓存
    /// 3. sticker 需下载转 base64，其余直传 Telegram CDN URL
    pub async fn analyze_attachment(
        &self,
        attachment_uuid: Uuid,
        prompt: Option<&str>,
    ) -> Result<String> {
        let (_, part) = self
            .0
            .db_srv
            .message
            .find_by_attachment_id(attachment_uuid)
            .await?
            .ok_or_err_msg(
                ErrorKind::NotFound,
                format!("attachment_id {} 不存在", attachment_uuid),
            )?;
        let parser = part
            .attachment_parser()
            .ok_or_err_msg(ErrorKind::BadRequest, "该附件类型不支持解析")?;
        let file_id = part
            .file_id()
            .ok_or_err_msg(ErrorKind::BadRequest, "附件缺少 file_id")?
            .to_string();

        // 从 file_id 确定性 UUID + Source::Platform，按 file_id 去重
        let source = Source::platform("telegram", &file_id);
        let is_sticker = matches!(part, TelegramContentPart::Sticker { .. });

        self.with_perception_cache(source, parser, prompt, async move {
            // Sticker 需要下载转 base64，其余类型直传 CDN URL
            if is_sticker {
                let file_data = self.download_telegram(&file_id).await?;
                let b64 = BASE64_STANDARD.encode(&file_data);
                self.0
                    .multimodal
                    .analyze_image(MediaInput::Base64(b64), prompt)
                    .await
            } else {
                let file_url = self.0.telegram_file.get_file_url(&file_id).await?;
                match parser {
                    AttachmentParser::Image => {
                        self.0
                            .multimodal
                            .analyze_image(MediaInput::Url(file_url), prompt)
                            .await
                    }
                    AttachmentParser::Ocr => self.0.multimodal.ocr(MediaInput::Url(file_url)).await,
                    AttachmentParser::Video => {
                        self.0
                            .multimodal
                            .analyze_video(MediaInput::Url(file_url), prompt)
                            .await
                    }
                    AttachmentParser::Audio => {
                        let fmt = part
                            .media_format()
                            .unwrap_or(MediaCodec::default_audio())
                            .ext();
                        self.0
                            .multimodal
                            .analyze_audio(MediaInput::Url(file_url), prompt, Some(fmt))
                            .await
                    }
                }
            }
        })
        .await
    }

    /// 直接分析 URL 资源，不下载。适用于用户发送的链接（图片、视频等）。
    /// perception 以 Source::Url { url } 缓存。
    pub async fn analyze_url(
        &self,
        url: &str,
        parser: AttachmentParser,
        prompt: Option<&str>,
    ) -> Result<String> {
        let source = Source::url(url);
        let owned_url = url.to_owned();

        self.with_perception_cache(source, parser, prompt, async move {
            match parser {
                AttachmentParser::Image => {
                    self.0
                        .multimodal
                        .analyze_image(MediaInput::Url(owned_url), prompt)
                        .await
                }
                AttachmentParser::Ocr => self.0.multimodal.ocr(MediaInput::Url(owned_url)).await,
                AttachmentParser::Video => {
                    self.0
                        .multimodal
                        .analyze_video(MediaInput::Url(owned_url), prompt)
                        .await
                }
                AttachmentParser::Audio => {
                    self.0
                        .multimodal
                        .analyze_audio(MediaInput::Url(owned_url), prompt, None)
                        .await
                }
            }
        })
        .await
    }
}
